//! Host-only model credentials in the current Windows user's Credential Manager.
//!
//! This module exposes no IPC or guest endpoint. A UUID reference is the only
//! value that belongs in a provider manifest or a persisted control snapshot.
use crate::broker::{BrokerError, CredentialId, CredentialStore, Secret};
use std::sync::Arc;
#[cfg(any(windows, test))]
use zeroize::Zeroize;
use zeroize::Zeroizing;

const TARGET_PREFIX: &str = "step-one/model/";
const MAX_BLOB_BYTES: usize = 2560;

#[derive(Clone, Copy)]
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
enum NativeError {
    Denied,
    Unavailable,
    Corrupt,
    Other,
}

type NativeResult<T> = std::result::Result<T, NativeError>;

/// The narrow native seam allows tests to exercise storage without inspecting or
/// changing any real user's vault. The returned bytes own their wiping lifetime.
trait CredentialApi: Send + Sync {
    fn read(&self, target: &[u16]) -> NativeResult<Option<Zeroizing<Vec<u8>>>>;
    fn write(&self, target: &[u16], value: &[u8]) -> NativeResult<()>;
    fn remove(&self, target: &[u16]) -> NativeResult<()>;
}

pub struct WindowsCredentialStore {
    api: Arc<dyn CredentialApi>,
}

impl Default for WindowsCredentialStore {
    fn default() -> Self {
        Self::new()
    }
}

impl WindowsCredentialStore {
    pub fn new() -> Self {
        Self {
            api: Arc::new(SystemCredentialApi),
        }
    }

    /// Called only by trusted host onboarding; no guest can choose a target name.
    pub fn set(&self, id: &CredentialId, secret: &Secret) -> Result<(), BrokerError> {
        let value = secret.expose_secret().as_bytes();
        validate_blob(value)?;
        let buffer = Zeroizing::new(value.to_vec());
        self.api
            .write(&target_name(id), &buffer)
            .map_err(public_error)
    }

    /// Removing an already absent application credential is idempotent.
    pub fn remove(&self, id: &CredentialId) -> Result<(), BrokerError> {
        self.api.remove(&target_name(id)).map_err(public_error)
    }
}

impl CredentialStore for WindowsCredentialStore {
    fn resolve(&self, id: &CredentialId) -> Result<Secret, BrokerError> {
        let bytes = self
            .api
            .read(&target_name(id))
            .map_err(public_error)?
            .ok_or_else(|| BrokerError::new("CREDENTIAL_MISSING", "请先在宿主机连接模型服务。"))?;
        validate_blob(&bytes)?;
        let value = std::str::from_utf8(&bytes).map_err(|_| {
            BrokerError::new(
                "CREDENTIAL_INVALID",
                "已保存的凭据无法使用，请在宿主机重新配置。",
            )
        })?;
        // Secret immediately assumes ownership of the temporary String and wipes
        // its allocation on drop. `bytes` is independently wiped on every exit.
        Secret::new(value.to_owned()).map_err(|_| {
            BrokerError::new(
                "CREDENTIAL_INVALID",
                "已保存的凭据无法使用，请在宿主机重新配置。",
            )
        })
    }
}

fn target_name(id: &CredentialId) -> Vec<u16> {
    format!("{TARGET_PREFIX}{}", id.as_str())
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

fn validate_blob(value: &[u8]) -> Result<(), BrokerError> {
    if value.is_empty()
        || value.len() > MAX_BLOB_BYTES
        || value.iter().any(|byte| matches!(byte, 0 | b'\r' | b'\n'))
    {
        return Err(BrokerError::new(
            "CREDENTIAL_INVALID",
            "凭据必须为不含换行的非空值，且不超过系统存储限制。",
        ));
    }
    Ok(())
}

fn public_error(error: NativeError) -> BrokerError {
    match error {
        NativeError::Denied => {
            BrokerError::new("CREDENTIAL_ACCESS_DENIED", "当前用户无法访问凭据存储。")
        }
        NativeError::Unavailable => BrokerError::new(
            "CREDENTIAL_STORE_UNAVAILABLE",
            "此环境无法使用 Windows 凭据存储。",
        ),
        NativeError::Corrupt => BrokerError::new(
            "CREDENTIAL_INVALID",
            "已保存的凭据无法使用，请在宿主机重新配置。",
        ),
        NativeError::Other => {
            BrokerError::new("CREDENTIAL_STORE_FAILED", "凭据存储操作失败，请稍后重试。")
        }
    }
}

#[cfg(any(windows, test))]
fn wipe_bytes(bytes: &mut [u8]) {
    bytes.zeroize();
}

struct SystemCredentialApi;

#[cfg(windows)]
mod native {
    use super::*;
    use std::ptr;
    use windows_sys::Win32::Foundation::{
        GetLastError, ERROR_ACCESS_DENIED, ERROR_NOT_FOUND, ERROR_NO_SUCH_LOGON_SESSION,
    };
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    fn last_error(code: u32) -> NativeError {
        match code {
            ERROR_ACCESS_DENIED => NativeError::Denied,
            ERROR_NO_SUCH_LOGON_SESSION => NativeError::Unavailable,
            _ => NativeError::Other,
        }
    }

    /// Owns memory returned by CredReadW, including the native secret buffer.
    struct CredentialBlock(*mut CREDENTIALW);

    impl Drop for CredentialBlock {
        fn drop(&mut self) {
            // SAFETY: CredReadW returned this allocation; it remains owned until
            // CredFree. CredentialBlob points to the documented writable blob.
            unsafe {
                let credential = &mut *self.0;
                if !credential.CredentialBlob.is_null() && credential.CredentialBlobSize > 0 {
                    wipe_bytes(std::slice::from_raw_parts_mut(
                        credential.CredentialBlob,
                        credential.CredentialBlobSize as usize,
                    ));
                }
                CredFree(self.0.cast());
            }
        }
    }

    impl CredentialApi for SystemCredentialApi {
        fn read(&self, target: &[u16]) -> NativeResult<Option<Zeroizing<Vec<u8>>>> {
            let mut pointer = ptr::null_mut();
            // SAFETY: target_name creates a NUL-terminated UTF-16 name, and the
            // writable out-pointer lives until this synchronous API returns.
            let success = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut pointer) };
            if success == 0 {
                let code = unsafe { GetLastError() };
                return if code == ERROR_NOT_FOUND {
                    Ok(None)
                } else {
                    Err(last_error(code))
                };
            }
            if pointer.is_null() {
                return Err(NativeError::Corrupt);
            }
            let allocation = CredentialBlock(pointer);
            // SAFETY: allocation owns a non-null CREDENTIALW returned by Windows.
            let credential = unsafe { &*allocation.0 };
            let length = credential.CredentialBlobSize as usize;
            if credential.Type != CRED_TYPE_GENERIC
                || length == 0
                || length > MAX_BLOB_BYTES
                || credential.CredentialBlob.is_null()
            {
                return Err(NativeError::Corrupt);
            }
            // SAFETY: Windows reports this many bytes in its owned blob. The copy
            // gets a wiping owner before the native allocation is released.
            let bytes = unsafe { std::slice::from_raw_parts(credential.CredentialBlob, length) };
            Ok(Some(Zeroizing::new(bytes.to_vec())))
        }

        fn write(&self, target: &[u16], value: &[u8]) -> NativeResult<()> {
            let mut native_buffer = Zeroizing::new(value.to_vec());
            // SAFETY: CREDENTIALW is a plain Win32 C structure with all-zero
            // optional pointers/flags. Every mandatory field is set below.
            let mut credential: CREDENTIALW = unsafe { std::mem::zeroed() };
            credential.Type = CRED_TYPE_GENERIC;
            credential.TargetName = target.as_ptr() as *mut u16;
            credential.CredentialBlobSize = native_buffer.len() as u32;
            credential.CredentialBlob = native_buffer.as_mut_ptr();
            credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
            // LOCAL_MACHINE is persistence for this user's vault across logins;
            // it does not create a machine-wide credential accessible by others.
            // SAFETY: target and blob remain live for the synchronous write call.
            if unsafe { CredWriteW(&credential, 0) } == 0 {
                return Err(last_error(unsafe { GetLastError() }));
            }
            Ok(())
        }

        fn remove(&self, target: &[u16]) -> NativeResult<()> {
            // SAFETY: the target is the validated application's NUL-terminated name.
            if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } == 0 {
                let code = unsafe { GetLastError() };
                if code != ERROR_NOT_FOUND {
                    return Err(last_error(code));
                }
            }
            Ok(())
        }
    }
}

#[cfg(not(windows))]
impl CredentialApi for SystemCredentialApi {
    fn read(&self, _target: &[u16]) -> NativeResult<Option<Zeroizing<Vec<u8>>>> {
        Err(NativeError::Unavailable)
    }
    fn write(&self, _target: &[u16], _value: &[u8]) -> NativeResult<()> {
        Err(NativeError::Unavailable)
    }
    fn remove(&self, _target: &[u16]) -> NativeResult<()> {
        Err(NativeError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeApi {
        value: Mutex<Option<Zeroizing<Vec<u8>>>>,
        target: Mutex<Vec<u16>>,
        failure: Mutex<Option<NativeError>>,
    }

    impl CredentialApi for FakeApi {
        fn read(&self, target: &[u16]) -> NativeResult<Option<Zeroizing<Vec<u8>>>> {
            *self.target.lock().unwrap() = target.to_vec();
            if let Some(error) = *self.failure.lock().unwrap() {
                return Err(error);
            }
            Ok(self
                .value
                .lock()
                .unwrap()
                .as_ref()
                .map(|value| Zeroizing::new(value.to_vec())))
        }
        fn write(&self, target: &[u16], value: &[u8]) -> NativeResult<()> {
            *self.target.lock().unwrap() = target.to_vec();
            if let Some(error) = *self.failure.lock().unwrap() {
                return Err(error);
            }
            *self.value.lock().unwrap() = Some(Zeroizing::new(value.to_vec()));
            Ok(())
        }
        fn remove(&self, target: &[u16]) -> NativeResult<()> {
            *self.target.lock().unwrap() = target.to_vec();
            if let Some(error) = *self.failure.lock().unwrap() {
                return Err(error);
            }
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    fn id() -> CredentialId {
        CredentialId::new("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee").unwrap()
    }
    fn fixture() -> (WindowsCredentialStore, Arc<FakeApi>) {
        let api = Arc::new(FakeApi::default());
        (WindowsCredentialStore { api: api.clone() }, api)
    }

    #[test]
    fn stores_only_namespaced_uuid_and_roundtrips_utf8() {
        let (store, api) = fixture();
        let secret = Secret::new("private-token-凭据".to_owned()).unwrap();
        store.set(&id(), &secret).unwrap();
        assert_eq!(
            store.resolve(&id()).unwrap().expose_secret(),
            secret.expose_secret()
        );
        let target = api.target.lock().unwrap();
        assert_eq!(target.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&target[..target.len() - 1]).unwrap(),
            "step-one/model/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"
        );
    }

    #[test]
    fn missing_and_removal_are_safe_and_idempotent() {
        let (store, _) = fixture();
        assert_eq!(
            store.resolve(&id()).err().unwrap().code,
            "CREDENTIAL_MISSING"
        );
        store.remove(&id()).unwrap();
        store
            .set(&id(), &Secret::new("token".into()).unwrap())
            .unwrap();
        store.remove(&id()).unwrap();
        assert_eq!(
            store.resolve(&id()).err().unwrap().code,
            "CREDENTIAL_MISSING"
        );
    }

    #[test]
    fn empty_invalid_utf8_controls_and_oversized_native_values_are_refused() {
        let (store, api) = fixture();
        for value in [
            vec![],
            vec![0xff, 0xfe],
            b"a\0b".to_vec(),
            b"a\rb".to_vec(),
            b"a\nb".to_vec(),
            vec![b'x'; MAX_BLOB_BYTES + 1],
        ] {
            *api.value.lock().unwrap() = Some(Zeroizing::new(value));
            assert_eq!(
                store.resolve(&id()).err().unwrap().code,
                "CREDENTIAL_INVALID"
            );
        }
        let oversized = Secret::new("x".repeat(MAX_BLOB_BYTES + 1)).unwrap();
        assert_eq!(
            store.set(&id(), &oversized).err().unwrap().code,
            "CREDENTIAL_INVALID"
        );
    }

    #[test]
    fn maximum_blob_is_accepted_without_truncation() {
        let (store, _) = fixture();
        let value = "x".repeat(MAX_BLOB_BYTES);
        store
            .set(&id(), &Secret::new(value.clone()).unwrap())
            .unwrap();
        assert_eq!(store.resolve(&id()).unwrap().expose_secret(), value);
    }

    #[test]
    fn native_errors_have_fixed_safe_messages_for_all_operations() {
        let (store, api) = fixture();
        for (failure, code) in [
            (NativeError::Denied, "CREDENTIAL_ACCESS_DENIED"),
            (NativeError::Unavailable, "CREDENTIAL_STORE_UNAVAILABLE"),
            (NativeError::Corrupt, "CREDENTIAL_INVALID"),
            (NativeError::Other, "CREDENTIAL_STORE_FAILED"),
        ] {
            *api.failure.lock().unwrap() = Some(failure);
            let secret = Secret::new("must-not-appear".into()).unwrap();
            for error in [
                store.resolve(&id()).err().unwrap(),
                store.set(&id(), &secret).err().unwrap(),
                store.remove(&id()).err().unwrap(),
            ] {
                assert_eq!(error.code, code);
                assert!(!error.message.contains("must-not-appear"));
                assert!(!error.message.contains(id().as_str()));
            }
        }
    }

    #[test]
    fn native_wipe_clears_the_full_buffer() {
        let mut value = b"credentials-to-wipe".to_vec();
        wipe_bytes(&mut value);
        assert!(value.iter().all(|byte| *byte == 0));
    }

    #[cfg(not(windows))]
    #[test]
    fn other_platforms_fail_closed_without_fallback() {
        let store = WindowsCredentialStore::default();
        assert_eq!(
            store.resolve(&id()).err().unwrap().code,
            "CREDENTIAL_STORE_UNAVAILABLE"
        );
        assert_eq!(
            store
                .set(&id(), &Secret::new("token".into()).unwrap())
                .err()
                .unwrap()
                .code,
            "CREDENTIAL_STORE_UNAVAILABLE"
        );
        assert_eq!(
            store.remove(&id()).err().unwrap().code,
            "CREDENTIAL_STORE_UNAVAILABLE"
        );
    }
}
