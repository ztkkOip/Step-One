use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeCapabilities {
    mode: &'static str,
    available: bool,
    checks: Vec<RuntimeCheck>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct RuntimeCheck {
    id: &'static str,
    status: CheckStatus,
    message: &'static str,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
enum CheckStatus {
    Passed,
    Blocked,
    Unknown,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HostProbe {
    build: u32,
    sku: u32,
    is_workstation: bool,
    hypervisor_present: bool,
    firmware_virtualization: Option<bool>,
    hyper_v_module: bool,
    vm_service_running: bool,
    can_manage_hyper_v: bool,
}

fn check(id: &'static str, status: CheckStatus, message: &'static str) -> RuntimeCheck {
    RuntimeCheck {
        id,
        status,
        message,
    }
}

fn flag_check(
    id: &'static str,
    passed: bool,
    success: &'static str,
    failure: &'static str,
) -> RuntimeCheck {
    check(
        id,
        if passed {
            CheckStatus::Passed
        } else {
            CheckStatus::Blocked
        },
        if passed { success } else { failure },
    )
}

fn image_check(path: Option<&Path>) -> RuntimeCheck {
    let Some(path) = path else {
        return check(
            "image",
            CheckStatus::Blocked,
            "未配置 Linux VHDX 镜像。当前可继续使用模拟模式。",
        );
    };
    let is_vhdx = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("vhdx"));
    if !path.is_absolute() || !path.is_file() || !is_vhdx {
        return check(
            "image",
            CheckStatus::Blocked,
            "配置的镜像必须是存在的本机绝对路径 .vhdx 文件。",
        );
    }
    check(
        "image",
        CheckStatus::Unknown,
        "已找到 VHDX 文件；来源、完整性及 Linux 启动兼容性尚未验证。",
    )
}

fn report(probe: Option<HostProbe>, image: RuntimeCheck) -> RuntimeCapabilities {
    let mut checks = match probe {
        Some(probe) => {
            let is_supported_edition = probe.is_workstation
                && probe.build >= 22000
                && [4, 27, 48, 49, 125, 126, 161, 162, 164, 165].contains(&probe.sku);
            let virtualization = if probe.hypervisor_present {
                check(
                    "virtualization",
                    CheckStatus::Passed,
                    "系统虚拟机监控程序已运行。",
                )
            } else {
                match probe.firmware_virtualization {
                    Some(true) => {
                        check("virtualization", CheckStatus::Passed, "固件虚拟化已开启。")
                    }
                    Some(false) => check(
                        "virtualization",
                        CheckStatus::Blocked,
                        "未检测到固件虚拟化开启，请检查 BIOS/UEFI 设置。",
                    ),
                    None => check(
                        "virtualization",
                        CheckStatus::Unknown,
                        "无法确认固件虚拟化状态。",
                    ),
                }
            };
            vec![
                flag_check(
                    "windowsEdition",
                    is_supported_edition,
                    "Windows 11 Pro/Enterprise 版本符合要求。",
                    "真实运行时需要 Windows 11 Pro/Enterprise；当前系统版本不受支持。",
                ),
                virtualization,
                flag_check(
                    "hyperVModule",
                    probe.hyper_v_module,
                    "Hyper-V 管理模块已安装。",
                    "未检测到 Hyper-V 管理模块。",
                ),
                flag_check(
                    "vmService",
                    probe.vm_service_running,
                    "Hyper-V 虚拟机管理服务正在运行。",
                    "Hyper-V 虚拟机管理服务未运行或未安装。",
                ),
                flag_check(
                    "managementAccess",
                    probe.can_manage_hyper_v,
                    "当前账户可以查询 Hyper-V 主机。",
                    "当前账户无法查询 Hyper-V 主机，请检查安装及管理权限。",
                ),
            ]
        }
        None => vec![check(
            "hostProbe",
            CheckStatus::Unknown,
            "无法读取 Windows 主机条件，请稍后重新检查。",
        )],
    };
    checks.push(image);
    checks.push(check(
        "runtime",
        CheckStatus::Blocked,
        "真实沙箱尚未接入并完成验收；当前操作仅修改模拟状态，不提供隔离。",
    ));
    RuntimeCapabilities {
        mode: "simulator",
        available: false,
        checks,
    }
}

pub fn capabilities() -> RuntimeCapabilities {
    let configured_image = std::env::var_os("STEP_ONE_HYPERV_IMAGE");
    let image = image_check(configured_image.as_deref().map(Path::new));
    #[cfg(windows)]
    let probe = probe_host();
    #[cfg(not(windows))]
    let probe = None;
    report(probe, image)
}

#[cfg(windows)]
fn probe_host() -> Option<HostProbe> {
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let windows_root = executable.ancestors().nth(4)?;
    let mut child = Command::new(&executable)
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            HOST_PROBE,
        ])
        .env_clear()
        .env("SystemRoot", windows_root)
        .env("PATH", windows_root.join("System32"))
        .env("PSModulePath", executable.parent()?.join("Modules"))
        .creation_flags(0x08000000)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < Duration::from_secs(15) => {
                std::thread::sleep(Duration::from_millis(100))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    if !status.success() {
        return None;
    }
    let mut output = Vec::new();
    child
        .stdout
        .take()?
        .take(16384)
        .read_to_end(&mut output)
        .ok()?;
    serde_json::from_slice(&output).ok()
}

#[cfg(windows)]
const HOST_PROBE: &str = r#"
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$PSModuleAutoLoadingPreference = 'None'
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Management/Microsoft.PowerShell.Management.psd1')
Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Utility/Microsoft.PowerShell.Utility.psd1')
Import-Module (Join-Path $PSHOME 'Modules/CimCmdlets/CimCmdlets.psd1')
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$os = Get-CimInstance Win32_OperatingSystem
$computer = Get-CimInstance Win32_ComputerSystem
$processors = @(Get-CimInstance Win32_Processor)
$firmware = $null
if ($processors.Count -gt 0) {
    $known = @($processors | Where-Object { $null -ne $_.VirtualizationFirmwareEnabled })
    if ($known.Count -eq $processors.Count) {
        $firmware = @($processors | Where-Object { -not $_.VirtualizationFirmwareEnabled }).Count -eq 0
    }
}
$hypervModule = Join-Path $PSHOME 'Modules/Hyper-V/Hyper-V.psd1'
$module = [IO.File]::Exists($hypervModule)
$service = Get-Service vmms -ErrorAction SilentlyContinue
$access = $false
if ($module -and $null -ne $service -and $service.Status -eq 'Running') {
    try { Import-Module $hypervModule -ErrorAction Stop; Get-VMHost -ErrorAction Stop | Out-Null; $access = $true } catch { $access = $false }
}
[pscustomobject]@{
    build = [uint32]$os.BuildNumber
    sku = [uint32]$os.OperatingSystemSKU
    isWorkstation = $os.ProductType -eq 1
    hypervisorPresent = [bool]$computer.HypervisorPresent
    firmwareVirtualization = $firmware
    hyperVModule = $module
    vmServiceRunning = $null -ne $service -and $service.Status -eq 'Running'
    canManageHyperV = $access
} | ConvertTo-Json -Compress
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn supported_host() -> HostProbe {
        HostProbe {
            build: 22631,
            sku: 48,
            is_workstation: true,
            hypervisor_present: true,
            firmware_virtualization: Some(false),
            hyper_v_module: true,
            vm_service_running: true,
            can_manage_hyper_v: true,
        }
    }

    fn status<'a>(report: &'a RuntimeCapabilities, id: &str) -> &'a CheckStatus {
        &report
            .checks
            .iter()
            .find(|item| item.id == id)
            .unwrap()
            .status
    }

    #[test]
    fn running_hypervisor_does_not_require_firmware_flag() {
        let result = report(Some(supported_host()), image_check(None));
        assert_eq!(status(&result, "virtualization"), &CheckStatus::Passed);
    }

    #[test]
    fn home_edition_is_blocked_even_with_hypervisor_running() {
        let mut host = supported_host();
        host.sku = 100;
        let result = report(Some(host), image_check(None));
        assert_eq!(status(&result, "windowsEdition"), &CheckStatus::Blocked);
        assert!(!result.available);
    }

    #[test]
    fn older_windows_and_server_are_blocked() {
        for (build, is_workstation) in [(19045, true), (26100, false)] {
            let mut host = supported_host();
            host.build = build;
            host.is_workstation = is_workstation;
            assert_eq!(
                status(&report(Some(host), image_check(None)), "windowsEdition"),
                &CheckStatus::Blocked
            );
        }
    }

    #[test]
    fn probe_failure_does_not_report_missing_hardware() {
        let result = report(None, image_check(None));
        assert_eq!(status(&result, "hostProbe"), &CheckStatus::Unknown);
        assert_eq!(result.mode, "simulator");
    }

    #[test]
    fn missing_firmware_value_is_unknown() {
        let mut host = supported_host();
        host.hypervisor_present = false;
        host.firmware_virtualization = None;
        assert_eq!(
            status(&report(Some(host), image_check(None)), "virtualization"),
            &CheckStatus::Unknown
        );
    }

    #[test]
    fn valid_image_filename_never_implies_verified_runtime() {
        let root =
            std::env::temp_dir().join(format!("step-one-diagnostic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("image.VHDX");
        std::fs::write(&path, b"not a bootable image").unwrap();
        let result = report(Some(supported_host()), image_check(Some(&path)));
        assert_eq!(status(&result, "image"), &CheckStatus::Unknown);
        assert!(!result.available);
        assert_eq!(result.mode, "simulator");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(root).unwrap();
    }

    #[test]
    fn image_requires_existing_absolute_file_with_vhdx_extension() {
        for path in [
            Path::new("relative.vhdx"),
            Path::new("missing.iso"),
            Path::new("."),
        ] {
            assert_eq!(image_check(Some(path)).status, CheckStatus::Blocked);
        }
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "Read-only integration check of the current Windows host"]
    fn live_host_probe() {
        let probe = probe_host().expect("Windows probe must return valid JSON");
        let result = report(Some(probe), image_check(None));
        println!("{}", serde_json::to_string(&result).unwrap());
        assert!(!result.available);
    }
}
