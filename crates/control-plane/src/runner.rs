//! Bounded transport for the bundled Hyper-V management script.
//!
//! User input is JSON on stdin, never PowerShell source or command-line text.
//! The fixed script waits for a gate before its first instruction; the parent
//! attaches the process to a kill-on-close job before opening that gate.
use crate::hyperv::{HyperVRequest, HyperVResponse, HyperVRunner};
use crate::model::{ControlError, Result};

pub const MAX_REQUEST_BYTES: usize = 65_536;
pub const MAX_OUTPUT_BYTES: usize = 65_536;
pub const MAX_TIMEOUT_SECONDS: u64 = 120;
pub const MAX_SCRIPT_BYTES: usize = 131_072;

#[derive(Default)]
pub struct WindowsHyperVRunner;

impl HyperVRunner for WindowsHyperVRunner {
    fn invoke(&self, request: &HyperVRequest) -> Result<HyperVResponse> {
        validate_request(request)?;
        #[cfg(windows)]
        {
            run_script(include_str!("../scripts/Manage-StepOneVm.ps1"), request)
        }
        #[cfg(not(windows))]
        {
            Err(error("UNSUPPORTED_PLATFORM"))
        }
    }
}

fn error(code: &str) -> ControlError {
    let mut result = ControlError::new(code, "无法确认沙箱管理操作完成，请检查宿主机运行环境。");
    result.retryable = code == "TIMEOUT";
    result
}

fn validate_request(request: &HyperVRequest) -> Result<Vec<u8>> {
    if !(1..=MAX_TIMEOUT_SECONDS).contains(&request.timeout_seconds) {
        return Err(error("INVALID_REQUEST"));
    }
    let json = serde_json::to_vec(request).map_err(|_| error("INVALID_REQUEST"))?;
    if json.len() > MAX_REQUEST_BYTES {
        return Err(error("INVALID_REQUEST"));
    }
    Ok(json)
}

#[cfg(windows)]
fn encoded_bootstrap() -> String {
    use base64::Engine;
    // Script source is a bundled constant on a dedicated line, never request
    // data. Keeping only this fixed bootstrap on argv avoids Windows' 32 KiB
    // command line limit. No imports/probes/children run before the gate.
    let source = r#"[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false); if([Console]::In.ReadLine() -cne 'STEP_ONE_JOB_READY'){exit 125}; $s=[Console]::In.ReadLine(); if($s.Length -gt 174764){exit 125}; $b=[Convert]::FromBase64String($s); if($b.Length -gt 131072){exit 125}; & ([ScriptBlock]::Create([Text.Encoding]::UTF8.GetString($b)))"#;
    let bytes: Vec<u8> = source.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(windows)]
fn run_script(script: &str, request: &HyperVRequest) -> Result<HyperVResponse> {
    use base64::Engine;
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    let json = validate_request(request)?;
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(error("INVALID_SCRIPT"));
    }
    let script_line = base64::engine::general_purpose::STANDARD.encode(script);
    let windows = windows_directory()?;
    let binary = windows.join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let job = ProcessJob::new()?;
    let started = Instant::now();
    let mut command = Command::new(binary);
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-EncodedCommand",
            &encoded_bootstrap(),
        ])
        .env_clear()
        .env("SystemRoot", &windows)
        .current_dir(windows.join("System32"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW);
    // These are the only ambient values permitted into the management process.
    // Credential, agent, shell-profile and module-path variables are excluded.
    for name in ["TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command.spawn().map_err(|_| error("RUNNER_UNAVAILABLE"))?;
    if job.assign(&child).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error("PROCESS_CONTAINMENT_FAILED"));
    }
    let mut input = child
        .stdin
        .take()
        .ok_or_else(|| error("RUNNER_UNAVAILABLE"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| error("RUNNER_UNAVAILABLE"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let read_overflow = Arc::clone(&overflow);
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output
            .take((MAX_OUTPUT_BYTES + 1) as u64)
            .read_to_end(&mut bytes);
        if bytes.len() > MAX_OUTPUT_BYTES {
            read_overflow.store(true, Ordering::Release);
        }
        result.map(|_| bytes)
    });
    let writer = std::thread::spawn(move || {
        input.write_all(b"STEP_ONE_JOB_READY\n")?;
        input.write_all(script_line.as_bytes())?;
        input.write_all(b"\n")?;
        input.write_all(&json)
        // Dropping stdin signals ReadToEnd EOF to the immutable script.
    });
    let outcome = loop {
        if overflow.load(Ordering::Acquire) {
            break Err(error("OUTPUT_LIMIT"));
        }
        if started.elapsed() >= Duration::from_secs(request.timeout_seconds) {
            break Err(error("TIMEOUT"));
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break Err(error("RUNNER_UNAVAILABLE")),
        }
    };
    // Always kill descendants, including ones left behind by a successful
    // script. VM resources are managed separately; this job owns helpers only.
    drop(job);
    let _ = child.wait();
    let written = writer.join().map_err(|_| error("RUNNER_UNAVAILABLE"))?;
    let output = reader
        .join()
        .map_err(|_| error("RUNNER_UNAVAILABLE"))?
        .map_err(|_| error("RUNNER_UNAVAILABLE"))?;
    let status = outcome?;
    if output.len() > MAX_OUTPUT_BYTES {
        return Err(error("OUTPUT_LIMIT"));
    }
    written.map_err(|_| error("RUNNER_UNAVAILABLE"))?;
    let response: HyperVResponse =
        serde_json::from_slice(&output).map_err(|_| error("INVALID_RESPONSE"))?;
    // A structured failed response is valid on exit 1; success must agree with
    // the actual process exit. Neither stderr nor raw stdout enters an error.
    if !status.success() && response.ok {
        return Err(error("INVALID_RESPONSE"));
    }
    Ok(response)
}

#[cfg(windows)]
fn windows_directory() -> Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetWindowsDirectoryW;
    let mut buffer = vec![0u16; 32_768];
    // SAFETY: buffer is writable for exactly buffer.len() UTF-16 elements.
    let length = unsafe { GetWindowsDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if length == 0 || length >= buffer.len() {
        return Err(error("RUNNER_UNAVAILABLE"));
    }
    Ok(std::ffi::OsString::from_wide(&buffer[..length]).into())
}

#[cfg(windows)]
struct ProcessJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl ProcessJob {
    fn new() -> Result<Self> {
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        // SAFETY: null attributes/name request a private non-inheritable job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(error("PROCESS_CONTAINMENT_FAILED"));
        }
        let job = Self(handle);
        // SAFETY: the all-zero Windows information structure is valid.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: handle is owned; information pointer and size match the class.
        if unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(error("PROCESS_CONTAINMENT_FAILED"));
        }
        Ok(job)
    }
    fn assign(&self, child: &std::process::Child) -> Result<()> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        // SAFETY: both handles are live for the complete call.
        if unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle()) } == 0 {
            return Err(error("PROCESS_CONTAINMENT_FAILED"));
        }
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for ProcessJob {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a job handle. Closing it kills all
        // member processes because KILL_ON_JOB_CLOSE was configured before use.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hyperv::HyperVAction;
    fn request() -> HyperVRequest {
        HyperVRequest {
            action: HyperVAction::Probe,
            resource_id: None,
            image_path: std::path::PathBuf::from("C:/fixed/base.vhdx"),
            image_sha256: "a".repeat(64),
            base_root: std::path::PathBuf::from("C:/fixed/resources"),
            cpu_count: 2,
            memory_bytes: 2_147_483_648,
            disk_bytes: 34_359_738_368,
            timeout_seconds: 30,
        }
    }
    #[test]
    fn limits_are_validated_before_starting_any_child() {
        for seconds in [0, MAX_TIMEOUT_SECONDS + 1, u64::MAX] {
            let mut request = request();
            request.timeout_seconds = seconds;
            assert_eq!(
                validate_request(&request).unwrap_err().code,
                "INVALID_REQUEST"
            );
        }
        let mut oversized = request();
        oversized.image_sha256 = "x".repeat(MAX_REQUEST_BYTES);
        assert_eq!(
            validate_request(&oversized).unwrap_err().code,
            "INVALID_REQUEST"
        );
    }
    #[cfg(not(windows))]
    #[test]
    fn non_windows_is_explicitly_unavailable() {
        assert_eq!(
            WindowsHyperVRunner.invoke(&request()).unwrap_err().code,
            "UNSUPPORTED_PLATFORM"
        );
    }
    #[cfg(windows)]
    #[test]
    fn fixed_source_is_utf16_encoded_and_gated_before_the_body() {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded_bootstrap())
            .unwrap();
        let units: Vec<_> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|part| u16::from_le_bytes([part[0], part[1]]))
            .collect();
        let source = String::from_utf16(&units).unwrap();
        assert!(source.find("ReadLine").unwrap() < source.find("ScriptBlock").unwrap());
        assert!(!source.contains(&request().image_sha256));
        assert!(encoded_bootstrap().len() < 2_048);
    }
    #[cfg(windows)]
    #[test]
    fn powershell_receives_data_not_source_and_does_not_inherit_secrets() {
        let mut data = request();
        data.image_path = std::path::PathBuf::from("'; throw 'injected'");
        let script = r#"
$request = [Console]::In.ReadToEnd() | ConvertFrom-Json
if ($request.imagePath -ne "'; throw 'injected'") { exit 3 }
$unexpected = @(Get-ChildItem Env: | Where-Object { $_.Name -notin @('SystemRoot','TEMP','TMP','PSModulePath','PathEXT') })
@{ok=$true;available=$false;state='missing';owned=$false;checks=@($unexpected | ForEach-Object {$_.Name});errorCode=$null} | ConvertTo-Json -Compress
"#;
        let response = run_script(script, &data).unwrap();
        assert!(response.ok);
        assert!(!response.available);
        assert!(
            response.checks.is_empty(),
            "unexpected keys: {:?}",
            response.checks
        );
    }
    #[cfg(windows)]
    #[test]
    fn timeout_and_output_cap_are_bounded_and_do_not_echo_output() {
        let mut data = request();
        data.timeout_seconds = 1;
        let started = std::time::Instant::now();
        let timeout = run_script(
            "$null=[Console]::In.ReadToEnd(); Start-Sleep -Seconds 30",
            &data,
        )
        .unwrap_err();
        assert_eq!(timeout.code, "TIMEOUT");
        assert!(started.elapsed() < std::time::Duration::from_secs(8));
        data.timeout_seconds = 30;
        let output = run_script(
            "$null=[Console]::In.ReadToEnd(); [Console]::Out.Write(('SENSITIVE' * 10000))",
            &data,
        )
        .unwrap_err();
        assert_eq!(output.code, "OUTPUT_LIMIT");
        assert!(!output.message.contains("SENSITIVE"));
    }
    #[cfg(windows)]
    #[test]
    fn successful_parent_cannot_leave_a_child_process_running() {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        };
        let script = r#"
$null=[Console]::In.ReadToEnd()
$pi = New-Object System.Diagnostics.ProcessStartInfo
$pi.FileName = [IO.Path]::Combine([Environment]::GetFolderPath('System'), 'WindowsPowerShell\v1.0\powershell.exe')
$pi.Arguments = '-NoProfile -NonInteractive -Command Start-Sleep -Seconds 30'
$pi.UseShellExecute = $false
$pi.CreateNoWindow = $true
$p = [Diagnostics.Process]::Start($pi)
@{ok=$true;available=$false;state='missing';owned=$false;checks=@([string]$p.Id);errorCode=$null} | ConvertTo-Json -Compress
"#;
        let response = run_script(script, &request()).unwrap();
        let pid: u32 = response.checks[0].parse().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            // SAFETY: only querying a test-owned process handle; never terminate
            // by PID (which could have been recycled).
            let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if process.is_null() {
                break;
            }
            let mut exit_code = 0;
            // SAFETY: process is live and exit_code is writable for the call.
            let result = unsafe { GetExitCodeProcess(process, &mut exit_code) };
            unsafe {
                CloseHandle(process);
            }
            if result == 0 || exit_code != 259 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child survived the parent job"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    #[cfg(windows)]
    #[test]
    fn large_bundled_source_uses_stdin_without_a_large_command_line() {
        let mut script = format!("# {}\n", "safe comment".repeat(3000));
        script.push_str("$null=[Console]::In.ReadToEnd(); '{\"ok\":true,\"available\":false,\"state\":\"missing\",\"owned\":false,\"checks\":[],\"errorCode\":null}'");
        assert!(script.len() > 32767);
        assert!(run_script(&script, &request()).unwrap().ok);
        assert_eq!(
            run_script(&" ".repeat(MAX_SCRIPT_BYTES + 1), &request())
                .unwrap_err()
                .code,
            "INVALID_SCRIPT"
        );
    }
    #[cfg(windows)]
    #[test]
    fn invalid_response_and_nonzero_success_are_rejected() {
        for script in [
            "$null=[Console]::In.ReadToEnd(); [Console]::Error.Write('secret'); 'invalid'",
            "$null=[Console]::In.ReadToEnd(); '{\"ok\":true,\"available\":false,\"state\":\"missing\",\"owned\":false,\"checks\":[],\"errorCode\":null}'; exit 1",
        ] {
            let result = run_script(script, &request()).unwrap_err();
            assert_eq!(result.code, "INVALID_RESPONSE");
            assert!(!result.message.contains("secret"));
        }
    }
}
