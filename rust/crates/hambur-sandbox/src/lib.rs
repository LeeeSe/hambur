use std::fs;
use std::path::{Component, Path, PathBuf};

use hambur_core::{HamburError, HamburResult};

const HAMBUR_PREFIX: &str = "/var/hambur";
const AUTOSTART_PREFIX: &str = "/var/minis/autostart";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxAccess {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SandboxPathResolution {
    pub sandbox_path: String,
    pub host_path: PathBuf,
    pub relative_path: String,
    pub root: String,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootfsStatus {
    pub available: bool,
    pub backend: String,
    pub abi: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct SandboxService {
    root: PathBuf,
    rootfs_status: RootfsStatus,
}

#[derive(Debug, Clone, Copy)]
struct VirtualRoot {
    sandbox_prefix: &'static str,
    host_prefix: &'static str,
    name: &'static str,
    session_scoped: bool,
    writable: bool,
}

const VIRTUAL_ROOTS: &[VirtualRoot] = &[
    VirtualRoot {
        sandbox_prefix: "/var/hambur/workspace",
        host_prefix: "workspace",
        name: "workspace",
        session_scoped: true,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/attachments",
        host_prefix: "attachments",
        name: "attachments",
        session_scoped: true,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/browser",
        host_prefix: "browser",
        name: "browser",
        session_scoped: true,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/mounts",
        host_prefix: "mounts",
        name: "mounts",
        session_scoped: true,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/offloads",
        host_prefix: "offloads",
        name: "offloads",
        session_scoped: true,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/shared",
        host_prefix: "shared",
        name: "shared",
        session_scoped: false,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/memory",
        host_prefix: "memory",
        name: "memory",
        session_scoped: false,
        writable: false,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/skills",
        host_prefix: "skills",
        name: "skills",
        session_scoped: false,
        writable: false,
    },
    VirtualRoot {
        sandbox_prefix: AUTOSTART_PREFIX,
        host_prefix: "autostart",
        name: "autostart",
        session_scoped: false,
        writable: false,
    },
];

impl SandboxService {
    pub fn new(app_files_dir: impl AsRef<Path>) -> HamburResult<Self> {
        let root = app_files_dir.as_ref().join("sandbox");
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create sandbox root: {error}")))?;
        Ok(Self {
            root,
            rootfs_status: RootfsStatus::for_target("proot"),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rootfs_status(&self) -> &RootfsStatus {
        &self.rootfs_status
    }

    pub fn resolve(
        &self,
        session_id: &str,
        sandbox_path: &str,
        access: SandboxAccess,
    ) -> HamburResult<SandboxPathResolution> {
        let session_id = normalize_session_id(session_id)?;
        let normalized = normalize_sandbox_path(sandbox_path)?;
        let (virtual_root, tail) = find_virtual_root(&normalized)?;
        let writable = virtual_root.writable && !is_read_only_attachment_uploads(&normalized);
        if matches!(access, SandboxAccess::Write) && !writable {
            return Err(HamburError::InvalidCommand(format!(
                "sandbox path is read-only: {normalized}"
            )));
        }

        let mut host_path = self.root.clone();
        if virtual_root.session_scoped {
            host_path.push("sessions");
            host_path.push(&session_id);
        } else {
            host_path.push("global");
        }
        host_path.push(virtual_root.host_prefix);
        for segment in &tail {
            host_path.push(segment);
        }
        ensure_inside_root(&self.root, &host_path)?;
        let relative_path = host_path
            .strip_prefix(&self.root)
            .unwrap_or(&host_path)
            .to_string_lossy()
            .replace('\\', "/");

        Ok(SandboxPathResolution {
            sandbox_path: normalized,
            host_path,
            relative_path,
            root: virtual_root.name.to_string(),
            writable,
        })
    }

    pub fn prepare_session(&self, session_id: &str) -> HamburResult<()> {
        for root in VIRTUAL_ROOTS.iter().filter(|root| root.session_scoped) {
            let resolved = self.resolve(session_id, root.sandbox_prefix, SandboxAccess::Read)?;
            fs::create_dir_all(&resolved.host_path).map_err(|error| {
                HamburError::Internal(format!(
                    "create sandbox session root {}: {error}",
                    resolved.host_path.display()
                ))
            })?;
        }
        Ok(())
    }
}

impl RootfsStatus {
    pub fn for_target(requested_backend: &str) -> Self {
        let backend = normalize_backend(requested_backend);
        let abi = android_abi();
        if abi == "arm64-v8a" {
            Self {
                available: true,
                backend,
                abi,
                reason: String::new(),
            }
        } else {
            Self {
                available: false,
                backend,
                abi,
                reason: "UnsupportedAbi".to_string(),
            }
        }
    }
}

fn normalize_backend(value: &str) -> String {
    match value.trim() {
        "chroot" => "chroot".to_string(),
        _ => "proot".to_string(),
    }
}

fn android_abi() -> String {
    if cfg!(target_os = "android") && cfg!(target_arch = "aarch64") {
        "arm64-v8a".to_string()
    } else if cfg!(target_os = "android") && cfg!(target_arch = "x86_64") {
        "x86_64".to_string()
    } else if cfg!(target_os = "android") && cfg!(target_arch = "arm") {
        "armeabi-v7a".to_string()
    } else {
        format!("host-{}", std::env::consts::ARCH)
    }
}

fn normalize_session_id(session_id: &str) -> HamburResult<String> {
    let session_id = session_id.trim();
    if session_id.is_empty()
        || session_id.contains('/')
        || session_id.contains('\\')
        || session_id == "."
        || session_id == ".."
    {
        return Err(HamburError::InvalidCommand(
            "session_id must be a safe sandbox segment".to_string(),
        ));
    }
    Ok(session_id.chars().take(160).collect())
}

fn normalize_sandbox_path(path: &str) -> HamburResult<String> {
    let path = path.trim();
    if path.is_empty() {
        return Err(HamburError::InvalidCommand(
            "sandbox path must not be empty".to_string(),
        ));
    }
    if !path.starts_with('/') {
        return Err(HamburError::InvalidCommand(
            "sandbox path must be absolute".to_string(),
        ));
    }
    if !(path == HAMBUR_PREFIX
        || path.starts_with(&format!("{HAMBUR_PREFIX}/"))
        || path == AUTOSTART_PREFIX
        || path.starts_with(&format!("{AUTOSTART_PREFIX}/")))
    {
        return Err(HamburError::InvalidCommand(format!(
            "unsupported sandbox root: {path}"
        )));
    }

    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => {}
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.is_empty() {
                    continue;
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::Prefix(_) => {
                return Err(HamburError::InvalidCommand(
                    "sandbox path must not escape through parent segments".to_string(),
                ));
            }
        }
    }
    Ok(format!("/{}", parts.join("/")))
}

fn find_virtual_root(path: &str) -> HamburResult<(VirtualRoot, Vec<String>)> {
    let mut roots = VIRTUAL_ROOTS.to_vec();
    roots.sort_by_key(|root| std::cmp::Reverse(root.sandbox_prefix.len()));
    for root in roots {
        if path == root.sandbox_prefix
            || path
                .strip_prefix(root.sandbox_prefix)
                .is_some_and(|tail| tail.starts_with('/'))
        {
            let tail = path
                .strip_prefix(root.sandbox_prefix)
                .unwrap_or_default()
                .trim_start_matches('/')
                .split('/')
                .filter(|part| !part.is_empty())
                .map(ToString::to_string)
                .collect();
            return Ok((root, tail));
        }
    }
    Err(HamburError::InvalidCommand(format!(
        "unsupported sandbox root: {path}"
    )))
}

fn is_read_only_attachment_uploads(path: &str) -> bool {
    path == "/var/hambur/attachments/uploads"
        || path.starts_with("/var/hambur/attachments/uploads/")
}

fn ensure_inside_root(root: &Path, candidate: &Path) -> HamburResult<()> {
    let canonical_root = root
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize sandbox root: {error}")))?;

    if candidate.exists() {
        let canonical_candidate = candidate.canonicalize().map_err(|error| {
            HamburError::Internal(format!("canonicalize sandbox path candidate: {error}"))
        })?;
        if !canonical_candidate.starts_with(&canonical_root) {
            return Err(HamburError::InvalidCommand(
                "sandbox path escaped root through symlink".to_string(),
            ));
        }
        return Ok(());
    }

    if let Some(parent) = nearest_existing_parent(candidate) {
        let canonical_parent = parent.canonicalize().map_err(|error| {
            HamburError::Internal(format!("canonicalize sandbox path parent: {error}"))
        })?;
        if !canonical_parent.starts_with(&canonical_root) {
            return Err(HamburError::InvalidCommand(
                "sandbox path escaped root through symlink".to_string(),
            ));
        }
    }
    Ok(())
}

fn nearest_existing_parent(path: &Path) -> Option<PathBuf> {
    let mut current = path.parent();
    while let Some(path) = current {
        if path.exists() {
            return Some(path.to_path_buf());
        }
        current = path.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::symlink;

    use hambur_core::new_id;

    use super::*;

    #[test]
    fn workspace_paths_resolve_under_session_root() {
        let dir = temp_dir();
        let sandbox = SandboxService::new(&dir).expect("sandbox");
        let resolved = sandbox
            .resolve(
                "session_1",
                "/var/hambur/workspace/src/main.rs",
                SandboxAccess::Write,
            )
            .expect("resolve");
        assert_eq!(resolved.root, "workspace");
        assert!(resolved.writable);
        assert!(
            resolved
                .relative_path
                .contains("sessions/session_1/workspace")
        );
        assert!(resolved.host_path.starts_with(sandbox.root()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn traversal_and_host_paths_are_rejected() {
        let dir = temp_dir();
        let sandbox = SandboxService::new(&dir).expect("sandbox");
        assert!(
            sandbox
                .resolve(
                    "session_1",
                    "/var/hambur/workspace/../memory/secret.md",
                    SandboxAccess::Read,
                )
                .is_err()
        );
        assert!(
            sandbox
                .resolve("session_1", "/tmp/secret", SandboxAccess::Read)
                .is_err()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn read_only_projection_writes_are_rejected() {
        let dir = temp_dir();
        let sandbox = SandboxService::new(&dir).expect("sandbox");
        assert!(
            sandbox
                .resolve(
                    "session_1",
                    "/var/hambur/memory/MEMORY.md",
                    SandboxAccess::Write,
                )
                .is_err()
        );
        assert!(
            sandbox
                .resolve(
                    "session_1",
                    "/var/hambur/attachments/uploads/photo.png",
                    SandboxAccess::Write,
                )
                .is_err()
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn symlink_escape_is_rejected() {
        let dir = temp_dir();
        let sandbox = SandboxService::new(&dir).expect("sandbox");
        let workspace = sandbox
            .resolve("session_1", "/var/hambur/workspace", SandboxAccess::Read)
            .expect("workspace");
        fs::create_dir_all(&workspace.host_path).expect("create workspace");
        let outside = dir.join("outside");
        fs::create_dir_all(&outside).expect("outside");
        symlink(&outside, workspace.host_path.join("escape")).expect("symlink");

        assert!(
            sandbox
                .resolve(
                    "session_1",
                    "/var/hambur/workspace/escape/file.txt",
                    SandboxAccess::Read,
                )
                .is_err()
        );
        let _ = fs::remove_dir_all(dir);
    }

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(new_id("hambur_sandbox_test"))
    }
}
