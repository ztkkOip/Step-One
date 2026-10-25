//! Host-selected workspace grants. A path string from the guest is never a grant.
use crate::model::ControlError;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

type Result<T> = std::result::Result<T, ControlError>;

#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceGrant {
    pub id: String,
    pub path: String,
    /// Selecting a directory does not approve writes. The permission flow does.
    pub read_only: bool,
    identity: String,
}

/// Safe directory-picker response; the object identity stays in the host store.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceSelection {
    pub id: String,
    pub path: String,
    pub read_only: bool,
}

impl WorkspaceGrant {
    pub fn public(&self) -> WorkspaceSelection {
        WorkspaceSelection {
            id: self.id.clone(),
            path: self.path.clone(),
            read_only: self.read_only,
        }
    }
}

pub struct WorkspaceRegistry {
    grants: BTreeMap<String, WorkspaceGrant>,
    home: Option<PathBuf>,
    sensitive_roots: Vec<PathBuf>,
}

impl Default for WorkspaceRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl WorkspaceRegistry {
    pub fn new() -> Self {
        let home =
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
        let sensitive_roots = ["APPDATA", "LOCALAPPDATA", "PROGRAMDATA", "SystemRoot"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        Self {
            grants: BTreeMap::new(),
            home,
            sensitive_roots,
        }
    }

    /// Supply explicit host locations in tests or a managed deployment.
    pub fn with_sensitive_roots(home: PathBuf, sensitive_roots: Vec<PathBuf>) -> Self {
        Self {
            grants: BTreeMap::new(),
            home: Some(home),
            sensitive_roots,
        }
    }

    pub fn restore(grants: Vec<WorkspaceGrant>) -> Self {
        Self {
            grants: grants
                .into_iter()
                .map(|grant| (grant.id.clone(), grant))
                .collect(),
            ..Self::new()
        }
    }

    pub fn grants(&self) -> Vec<WorkspaceGrant> {
        self.grants.values().cloned().collect()
    }

    pub fn grant(&mut self, path: &Path) -> Result<WorkspaceGrant> {
        let canonical = self.validate_path(path)?;
        let grant = WorkspaceGrant {
            id: Uuid::new_v4().to_string(),
            path: canonical.to_str().ok_or_else(denied)?.to_owned(),
            read_only: true,
            identity: directory_identity(&canonical)?,
        };
        self.grants.insert(grant.id.clone(), grant.clone());
        Ok(grant)
    }

    pub fn revalidate(&self, id: &str) -> Result<WorkspaceGrant> {
        let grant = self.grants.get(id).ok_or_else(|| {
            ControlError::new(
                "WORKSPACE_NOT_AUTHORIZED",
                "请通过宿主机目录选择器授权工作区。",
            )
        })?;
        let current = self.validate_path(Path::new(&grant.path))?;
        if current != Path::new(&grant.path) || directory_identity(&current)? != grant.identity {
            return Err(ControlError::new(
                "WORKSPACE_CHANGED",
                "工作区已被替换，请重新选择并授权。",
            ));
        }
        Ok(grant.clone())
    }

    fn validate_path(&self, path: &Path) -> Result<PathBuf> {
        if !path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, Component::ParentDir))
        {
            return Err(denied());
        }
        #[cfg(windows)]
        if !matches!(path.components().next(), Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
        {
            return Err(denied());
        }
        reject_links(path)?;
        let canonical = path.canonicalize().map_err(|_| denied())?;
        if canonical.parent().is_none() || !canonical.is_dir() {
            return Err(denied());
        }
        reject_links(&canonical)?;
        if canonical.components().any(|component| {
            let name = component.as_os_str().to_string_lossy().to_ascii_lowercase();
            [
                ".ssh",
                ".aws",
                ".azure",
                ".kube",
                ".gnupg",
                ".codex",
                ".claude",
                "appdata",
                ".mozilla",
                ".pki",
                "keychains",
                "google-chrome",
                "chromium",
            ]
            .contains(&name.as_str())
        }) {
            return Err(denied());
        }
        if let Some(home) = &self.home {
            let home = home.canonicalize().map_err(|_| denied())?;
            if contained(&home, &canonical) {
                // Reject the home directory itself and every parent containing it.
                return Err(denied());
            }
            let credentials = [".config", ".local/share/keyrings", "Library", "AppData"];
            if credentials
                .iter()
                .any(|child| contained(&canonical, &home.join(child)))
            {
                return Err(denied());
            }
        }
        for root in &self.sensitive_roots {
            if let Ok(root) = root.canonicalize() {
                if contained(&canonical, &root) || contained(&root, &canonical) {
                    return Err(denied());
                }
            }
        }
        Ok(canonical)
    }
}

pub fn grant(path: &Path) -> Result<WorkspaceGrant> {
    WorkspaceRegistry::new().grant(path)
}

pub fn revalidate(grant: &WorkspaceGrant) -> Result<PathBuf> {
    let registry = WorkspaceRegistry::restore(vec![grant.clone()]);
    registry
        .revalidate(&grant.id)
        .map(|grant| PathBuf::from(grant.path))
}

fn denied() -> ControlError {
    ControlError::new(
        "WORKSPACE_DENIED",
        "工作区不可用或包含受保护位置，请选择普通项目目录。",
    )
}

fn contained(path: &Path, root: &Path) -> bool {
    #[cfg(windows)]
    {
        let path: Vec<_> = path
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect();
        let root: Vec<_> = root
            .components()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase())
            .collect();
        path.starts_with(&root)
    }
    #[cfg(not(windows))]
    path.starts_with(root)
}

fn reject_links(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(|_| denied())?;
        if metadata.file_type().is_symlink() {
            return Err(denied());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                // Every reparse point is denied, including junctions and cloud placeholders.
                return Err(denied());
            }
        }
    }
    Ok(())
}

#[cfg(unix)]
fn directory_identity(path: &Path) -> Result<String> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).map_err(|_| denied())?;
    Ok(format!("{}:{}", metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn directory_identity(path: &Path) -> Result<String> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    #[repr(C)]
    #[derive(Default)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct FileInformation {
        attributes: u32,
        creation: FileTime,
        access: FileTime,
        write: FileTime,
        volume: u32,
        size_high: u32,
        size_low: u32,
        links: u32,
        index_high: u32,
        index_low: u32,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetFileInformationByHandle(
            handle: *mut std::ffi::c_void,
            information: *mut FileInformation,
        ) -> i32;
    }
    let file = std::fs::OpenOptions::new()
        .access_mode(0x80)
        .custom_flags(0x02000000 | 0x00200000)
        .open(path)
        .map_err(|_| denied())?;
    let mut info = FileInformation::default();
    // SAFETY: `file` keeps a valid handle open and `info` has the documented
    // BY_HANDLE_FILE_INFORMATION layout and remains writable for the call.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(denied());
    }
    if info.attributes & 0x400 != 0 {
        return Err(denied());
    }
    Ok(format!(
        "{}:{}:{}",
        info.volume, info.index_high, info.index_low
    ))
}

#[cfg(not(any(unix, windows)))]
fn directory_identity(_path: &Path) -> Result<String> {
    Err(ControlError::new(
        "UNSUPPORTED_PLATFORM",
        "此平台尚不支持安全工作区授权。",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory() -> tempfile::TempDir {
        // Windows' normal temporary directory is under AppData, which is
        // intentionally forbidden as an agent workspace.
        tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap()
    }

    fn registry(root: &Path) -> WorkspaceRegistry {
        let home = root.join("home");
        std::fs::create_dir_all(&home).unwrap();
        WorkspaceRegistry::with_sensitive_roots(home, vec![])
    }

    #[test]
    fn selected_workspace_is_read_only_and_survives_registry_roundtrip() {
        let temp = test_directory();
        let mut registry = registry(temp.path());
        let workspace = temp.path().join("project");
        std::fs::create_dir(&workspace).unwrap();
        let grant = registry.grant(&workspace).unwrap();
        assert!(grant.read_only);
        assert_eq!(registry.revalidate(&grant.id).unwrap(), grant);
        assert_eq!(registry.grants(), vec![grant]);
        assert!(registry.revalidate("invented-id").is_err());
    }

    #[test]
    fn rejects_root_home_ancestors_sensitive_and_traversal_paths() {
        let temp = test_directory();
        let mut registry = registry(temp.path());
        let home = temp.path().join("home");
        assert!(registry.grant(&home).is_err());
        assert!(registry.grant(temp.path()).is_err());
        assert!(registry.grant(Path::new("relative")).is_err());
        assert!(registry.grant(&temp.path().join("missing")).is_err());
        assert!(registry.grant(&home.join("..")).is_err());
        let file = temp.path().join("file");
        std::fs::write(&file, "data").unwrap();
        assert!(registry.grant(&file).is_err());
        for name in [
            ".ssh", ".aws", ".azure", ".kube", ".gnupg", ".codex", "AppData", ".config", "Library",
        ] {
            let sensitive = home.join(name).join("child");
            std::fs::create_dir_all(&sensitive).unwrap();
            assert!(registry.grant(&sensitive).is_err(), "{name}");
        }
    }

    #[test]
    fn replacing_selected_directory_revokes_its_identity() {
        let temp = test_directory();
        let mut registry = registry(temp.path());
        let workspace = temp.path().join("project");
        std::fs::create_dir(&workspace).unwrap();
        let grant = registry.grant(&workspace).unwrap();
        std::fs::rename(&workspace, temp.path().join("old-project")).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        assert!(registry.revalidate(&grant.id).is_err());
        std::fs::remove_dir(&workspace).unwrap();
        assert!(registry.revalidate(&grant.id).is_err());
    }

    #[test]
    fn extra_sensitive_locations_reject_descendants_and_parents() {
        let temp = test_directory();
        let home = temp.path().join("home");
        let sensitive = temp.path().join("system/credentials");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(sensitive.join("child")).unwrap();
        let mut registry = WorkspaceRegistry::with_sensitive_roots(home, vec![sensitive.clone()]);
        assert!(registry.grant(&sensitive).is_err());
        assert!(registry.grant(sensitive.parent().unwrap()).is_err());
        assert!(registry.grant(&sensitive.join("child")).is_err());
    }

    #[test]
    fn persisted_grants_are_revalidated_and_public_response_hides_identity() {
        let temp = test_directory();
        let mut registry = WorkspaceRegistry::new();
        let workspace = temp.path().join("project");
        std::fs::create_dir(&workspace).unwrap();
        let grant = registry.grant(&workspace).unwrap();
        let persisted = serde_json::to_string(&grant).unwrap();
        let reloaded: WorkspaceGrant = serde_json::from_str(&persisted).unwrap();
        assert_eq!(
            revalidate(&reloaded).unwrap(),
            workspace.canonicalize().unwrap()
        );
        let public = serde_json::to_value(grant.public()).unwrap();
        assert!(public.get("identity").is_none());
        assert_eq!(public["id"], grant.id);
        let mut invalid: serde_json::Value = serde_json::from_str(&persisted).unwrap();
        invalid["identity"] = "invalid-file-identity".into();
        let invalid: WorkspaceGrant = serde_json::from_value(invalid).unwrap();
        assert!(revalidate(&invalid).is_err());
        assert!(grant.path.contains("project"));
    }

    #[test]
    fn default_registry_and_free_grant_agree() {
        let temp = test_directory();
        let workspace = temp.path().join("project");
        std::fs::create_dir(&workspace).unwrap();
        let granted = grant(&workspace).unwrap();
        assert_eq!(
            revalidate(&granted).unwrap(),
            workspace.canonicalize().unwrap()
        );
    }

    #[cfg(unix)]
    #[test]
    fn denies_symlink_directory_and_symlink_ancestors() {
        let temp = test_directory();
        let mut registry = registry(temp.path());
        let workspace = temp.path().join("project");
        std::fs::create_dir_all(workspace.join("nested")).unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&workspace, &link).unwrap();
        assert!(registry.grant(&link).is_err());
        assert!(registry.grant(&link.join("nested")).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn denies_junctions_and_paths_beneath_junctions() {
        let temp = test_directory();
        let mut registry = registry(temp.path());
        let workspace = temp.path().join("project");
        std::fs::create_dir_all(workspace.join("nested")).unwrap();
        let link = temp.path().join("link");
        let result = std::process::Command::new("powershell.exe")
            .args(["-NoProfile", "-NonInteractive", "-Command",
                "$ErrorActionPreference='Stop'; New-Item -ItemType Junction -Path $env:STEP_ONE_TEST_LINK -Target $env:STEP_ONE_TEST_TARGET | Out-Null"])
            .env("STEP_ONE_TEST_LINK", &link).env("STEP_ONE_TEST_TARGET", &workspace)
            .output().unwrap();
        assert!(
            result.status.success(),
            "junction fixture failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(registry.grant(&link).is_err());
        assert!(registry.grant(&link.join("nested")).is_err());
    }
}
