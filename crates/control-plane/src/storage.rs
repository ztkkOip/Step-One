//! Transactional, host-only control-plane state. No credential values belong here.
use crate::model::{ControlError, PersistedState};
use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

const SCHEMA_VERSION: u32 = 1;
type Result<T> = std::result::Result<T, ControlError>;

pub struct Store {
    connection: Mutex<Connection>,
    // This lease outlives the SQLite connection. Only one control service may
    // publish whole snapshots or own runtime operations for this database.
    _lease: std::fs::File,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|_| storage_error())?;
        }
        let mut lease_name = path.as_os_str().to_os_string();
        lease_name.push(".lock");
        let lease_path = std::path::PathBuf::from(lease_name);
        if let Ok(metadata) = std::fs::symlink_metadata(&lease_path) {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(storage_error());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(storage_error());
                }
            }
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lease = options.open(&lease_path).map_err(|_| storage_error())?;
        lease.try_lock_exclusive().map_err(|_| {
            ControlError::new(
                "STATE_IN_USE",
                "此工作区控制服务已经运行，请使用已打开的应用。",
            )
            .retryable()
        })?;
        if path.exists() {
            let metadata = std::fs::symlink_metadata(path).map_err(|_| storage_error())?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(storage_error());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(storage_error());
                }
            }
        } else {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            options.open(path).map_err(|_| storage_error())?;
        }
        let mut connection = Connection::open(path).map_err(|_| storage_error())?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|_| storage_error())?;
        connection
            .execute_batch("PRAGMA trusted_schema = OFF; PRAGMA foreign_keys = ON;")
            .map_err(|_| storage_error())?;
        let version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|_| storage_error())?;
        match version {
            0 => {
                let tables: u32 = connection.query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'", [],
                    |row| row.get(0),
                ).map_err(|_| storage_error())?;
                if tables != 0 {
                    return Err(schema_error());
                }
                let transaction = connection.transaction().map_err(|_| storage_error())?;
                transaction.execute_batch(
                    "CREATE TABLE control_state (id INTEGER PRIMARY KEY CHECK(id = 1), payload TEXT NOT NULL); PRAGMA user_version = 1;",
                ).map_err(|_| storage_error())?;
                transaction.commit().map_err(|_| storage_error())?;
            }
            SCHEMA_VERSION => {}
            _ => return Err(schema_error()),
        }
        Ok(Self {
            connection: Mutex::new(connection),
            _lease: lease,
        })
    }

    pub fn load(&self) -> Result<Option<PersistedState>> {
        let connection = self.lock()?;
        let payload: Option<String> = connection
            .query_row(
                "SELECT payload FROM control_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|_| storage_error())?;
        payload
            .map(|payload| {
                let state: PersistedState = serde_json::from_str(&payload).map_err(|_| {
                    ControlError::new(
                        "STATE_CORRUPT",
                        "本地状态无法验证，请保留数据并联系维护者。",
                    )
                })?;
                if state.schema_version != SCHEMA_VERSION {
                    return Err(schema_error());
                }
                Ok(state)
            })
            .transpose()
    }

    pub fn save(&self, state: &PersistedState) -> Result<()> {
        if state.schema_version != SCHEMA_VERSION {
            return Err(schema_error());
        }
        let payload = serde_json::to_string(state).map_err(|_| storage_error())?;
        let mut connection = self.lock()?;
        let transaction = connection.transaction().map_err(|_| storage_error())?;
        transaction.execute(
            "INSERT INTO control_state(id, payload) VALUES(1, ?1) ON CONFLICT(id) DO UPDATE SET payload = excluded.payload",
            params![payload],
        ).map_err(|_| storage_error())?;
        transaction.commit().map_err(|_| storage_error())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection.lock().map_err(|_| storage_error())
    }
}

fn storage_error() -> ControlError {
    ControlError::new(
        "STORAGE_UNAVAILABLE",
        "无法读取或提交本地状态；操作未确认完成。",
    )
}

fn schema_error() -> ControlError {
    ControlError::new(
        "STATE_VERSION_UNSUPPORTED",
        "本地状态版本不兼容，请使用匹配版本的应用。",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> PersistedState {
        use crate::model::{Console, RuntimeInfo};
        PersistedState {
            schema_version: SCHEMA_VERSION,
            console: Console::new(RuntimeInfo {
                kind: "simulator".into(),
                available: true,
                isolation_enforced: false,
                capabilities: Default::default(),
                message: "test".into(),
            }),
            grants: vec![],
        }
    }

    #[test]
    fn empty_database_and_restart_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested/state.sqlite");
        let store = Store::open(&path).unwrap();
        assert!(store.load().unwrap().is_none());
        let expected = state();
        store.save(&expected).unwrap();
        store.save(&expected).unwrap();
        drop(store);
        let actual = Store::open(&path).unwrap().load().unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }

    #[test]
    fn unsupported_schema_and_unversioned_existing_tables_fail_closed() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("future.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 999;")
            .unwrap();
        drop(connection);
        assert_eq!(
            Store::open(&path).err().unwrap().code,
            "STATE_VERSION_UNSUPPORTED"
        );
        let path = temp.path().join("unknown.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("CREATE TABLE unexpected(secret TEXT);")
            .unwrap();
        drop(connection);
        assert_eq!(
            Store::open(&path).err().unwrap().code,
            "STATE_VERSION_UNSUPPORTED"
        );
    }

    #[test]
    fn corrupted_file_and_payload_never_expose_contents() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("broken.sqlite");
        std::fs::write(&path, "secret-should-not-appear").unwrap();
        let error = Store::open(&path).err().unwrap();
        assert!(!error.message.contains("secret-should-not-appear"));
        let path = temp.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "INSERT INTO control_state VALUES(1, ?1)",
                ["sensitive-invalid-json"],
            )
            .unwrap();
        let error = store.load().err().unwrap();
        assert_eq!(error.code, "STATE_CORRUPT");
        assert!(!error.message.contains("sensitive-invalid-json"));
    }

    #[test]
    fn unsupported_payload_and_save_do_not_overwrite_valid_state() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store.save(&state()).unwrap();
        let mut future = state();
        future.schema_version = 2;
        assert_eq!(
            store.save(&future).err().unwrap().code,
            "STATE_VERSION_UNSUPPORTED"
        );
        assert_eq!(store.load().unwrap().unwrap().schema_version, 1);
        let connection = Connection::open(&path).unwrap();
        connection
            .execute(
                "UPDATE control_state SET payload = ?1",
                [serde_json::to_string(&future).unwrap()],
            )
            .unwrap();
        assert_eq!(
            store.load().err().unwrap().code,
            "STATE_VERSION_UNSUPPORTED"
        );
    }

    #[test]
    fn rollback_keeps_previous_state_on_write_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.sqlite");
        let store = Store::open(&path).unwrap();
        store.save(&state()).unwrap();
        let connection = Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TRIGGER fail_update BEFORE UPDATE ON control_state BEGIN SELECT RAISE(ABORT, 'internal secret'); END;").unwrap();
        let error = store.save(&state()).err().unwrap();
        assert!(!error.message.contains("internal secret"));
        assert_eq!(store.load().unwrap().unwrap().schema_version, 1);
    }

    #[test]
    fn refuses_directory_as_database() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("database");
        std::fs::create_dir(&directory).unwrap();
        assert!(Store::open(&directory).is_err());
    }

    #[test]
    fn invalid_parent_and_missing_versioned_table_fail_safely() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("not-a-directory");
        std::fs::write(&parent, "file").unwrap();
        assert_eq!(
            Store::open(&parent.join("state.sqlite"))
                .err()
                .unwrap()
                .code,
            "STORAGE_UNAVAILABLE"
        );
        let path = temp.path().join("state.sqlite");
        let connection = Connection::open(&path).unwrap();
        connection
            .execute_batch("PRAGMA user_version = 1;")
            .unwrap();
        drop(connection);
        let store = Store::open(&path).unwrap();
        assert_eq!(store.load().err().unwrap().code, "STORAGE_UNAVAILABLE");
    }

    #[test]
    fn poisoned_connection_returns_safe_error() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(&temp.path().join("state.sqlite")).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = store.connection.lock().unwrap();
            panic!("test-only connection interruption");
        }));
        assert_eq!(store.load().err().unwrap().code, "STORAGE_UNAVAILABLE");
        assert_eq!(
            store.save(&state()).err().unwrap().code,
            "STORAGE_UNAVAILABLE"
        );
    }

    #[test]
    fn one_service_owns_database_until_drop() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("state.sqlite");
        let first = Store::open(&path).unwrap();
        assert_eq!(Store::open(&path).err().unwrap().code, "STATE_IN_USE");
        drop(first);
        assert!(Store::open(&path).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_linked_database_or_lease() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        std::fs::write(&target, "must remain").unwrap();
        let path = temp.path().join("state.sqlite");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(Store::open(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(temp.path().join("state.sqlite.lock")).unwrap();
        std::os::unix::fs::symlink(&target, temp.path().join("state.sqlite.lock")).unwrap();
        assert!(Store::open(&path).is_err());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "must remain");
    }
}
