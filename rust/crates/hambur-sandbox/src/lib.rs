use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use tar::Archive;

use hambur_core::{HamburError, HamburResult};

const HAMBUR_PREFIX: &str = "/var/hambur";
const AUTOSTART_PREFIX: &str = "/var/minis/autostart";
const PROBE_CACHE_TTL_MS: u64 = 30_000;

const ROOTFS_RELEASES_BASE_URLS: &[&str] = &[
    "https://mirrors.tuna.tsinghua.edu.cn/alpine/latest-stable/releases/aarch64/",
    "https://mirrors.bfsu.edu.cn/alpine/latest-stable/releases/aarch64/",
    "https://mirrors.ustc.edu.cn/alpine/latest-stable/releases/aarch64/",
    "https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/aarch64/",
    "https://dl-2.alpinelinux.org/alpine/latest-stable/releases/aarch64/",
    "https://dl-3.alpinelinux.org/alpine/latest-stable/releases/aarch64/",
    "https://mirror.xtom.com.hk/alpine/latest-stable/releases/aarch64/",
];

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootfsStatus {
    pub available: bool,
    pub backend: String,
    pub abi: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupTask {
    pub id: String,
    pub name: String,
    pub script: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxExecResult {
    pub backend: String,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub elapsed_ms: u64,
    pub cwd: String,
    pub session_id: String,
    pub fallback_from: Option<String>,
    pub warning: Option<String>,
}

struct DownloadedRootfsArchive {
    file: PathBuf,
    version_label: String,
}

struct AlpineRootfsRelease {
    file_name: String,
    version: String,
    sha256: String,
}

#[derive(Debug, Clone)]
pub struct SandboxService {
    root: PathBuf, // sandbox_root
    app_files_dir: PathBuf,
    native_library_dir: Option<PathBuf>,
    rootfs_dir: PathBuf,
    rootfs_status: Arc<Mutex<RootfsStatus>>,
    probe_cache: Arc<Mutex<ProbeCache>>,
    chroot_mount_state: Arc<Mutex<ChrootMountState>>,
    chroot_exec_lock: Arc<Mutex<()>>,
    startup_tasks_ran: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, Default)]
struct ProbeCache {
    root: Option<TimedProbe>,
    chroot: Option<TimedProbe>,
    proot: Option<TimedProbe>,
}

#[derive(Debug, Clone)]
struct TimedProbe {
    available: bool,
    checked_at_ms: u64,
}

impl TimedProbe {
    fn is_fresh(&self) -> bool {
        now_ms().saturating_sub(self.checked_at_ms) < PROBE_CACHE_TTL_MS
    }
}

#[derive(Debug, Clone, Default)]
struct ChrootMountState {
    static_ready: bool,
    prepared_session_id: Option<String>,
}

#[derive(Debug, Clone)]
struct SessionPaths {
    session_id: String,
    attachments: PathBuf,
    uploads: PathBuf,
    browser: PathBuf,
    mounts: PathBuf,
    offloads: PathBuf,
    workspace: PathBuf,
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
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: "/var/hambur/download",
        host_prefix: "download",
        name: "download",
        session_scoped: false,
        writable: true,
    },
    VirtualRoot {
        sandbox_prefix: AUTOSTART_PREFIX,
        host_prefix: "var/minis/autostart",
        name: "autostart",
        session_scoped: false,
        writable: false,
    },
];

impl SandboxService {
    pub fn new(app_files_dir: impl AsRef<Path>) -> HamburResult<Self> {
        Self::new_with_native_library_dir(app_files_dir, "")
    }

    pub fn new_with_native_library_dir(
        app_files_dir: impl AsRef<Path>,
        native_library_dir: impl AsRef<Path>,
    ) -> HamburResult<Self> {
        let app_files_dir = app_files_dir.as_ref().to_path_buf();
        let native_library_dir = native_library_dir.as_ref();
        let native_library_dir = if native_library_dir.as_os_str().is_empty() {
            None
        } else {
            Some(native_library_dir.to_path_buf())
        };
        let root = app_files_dir.join("sandbox");
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create sandbox root: {error}")))?;
        let rootfs_dir = app_files_dir.join("alpine-rootfs");
        eprintln!(
            "RootfsDebug sandbox_new app_files_dir={} native_library_dir={} root={} rootfs_dir={} root_exists={} rootfs_exists={}",
            app_files_dir.display(),
            native_library_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            root.display(),
            rootfs_dir.display(),
            root.exists(),
            rootfs_dir.exists()
        );
        Ok(Self {
            root,
            app_files_dir,
            native_library_dir,
            rootfs_dir,
            rootfs_status: Arc::new(Mutex::new(RootfsStatus::for_target("proot"))),
            probe_cache: Arc::new(Mutex::new(ProbeCache::default())),
            chroot_mount_state: Arc::new(Mutex::new(ChrootMountState::default())),
            chroot_exec_lock: Arc::new(Mutex::new(())),
            startup_tasks_ran: Arc::new(Mutex::new(false)),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rootfs_dir(&self) -> &Path {
        &self.rootfs_dir
    }

    pub fn app_files_dir(&self) -> &Path {
        &self.app_files_dir
    }

    pub fn rootfs_status(&self) -> RootfsStatus {
        self.rootfs_status.lock().unwrap().clone()
    }

    pub fn resolve(
        &self,
        session_id: &str,
        sandbox_path: &str,
        access: SandboxAccess,
    ) -> HamburResult<SandboxPathResolution> {
        let normalized = normalize_sandbox_path(sandbox_path)?;
        let (virtual_root, tail) = find_virtual_root(&normalized)?;
        let writable = virtual_root.writable && !is_read_only_attachment_uploads(&normalized);
        if matches!(access, SandboxAccess::Write) && !writable {
            return Err(HamburError::InvalidCommand(format!(
                "sandbox path is read-only: {normalized}"
            )));
        }

        let mut host_path = self.root.clone();
        let guard_root: &Path;
        let android_download = Path::new("/storage/emulated/0/Download");
        if virtual_root.name == "download" && android_download.exists() {
            host_path = android_download.to_path_buf();
            guard_root = android_download;
            for segment in &tail {
                host_path.push(segment);
            }
            ensure_inside_root(guard_root, &host_path)?;
            let relative_path = format!("download/{}", tail.join("/"));
            return Ok(SandboxPathResolution {
                sandbox_path: normalized,
                host_path,
                relative_path,
                root: virtual_root.name.to_string(),
                writable,
            });
        }
        if virtual_root.session_scoped {
            let session_id = normalize_session_id(session_id)?;
            host_path.push("sessions");
            host_path.push(&session_id);
            guard_root = &self.root;
        } else {
            if virtual_root.host_prefix.contains('/') {
                host_path = self.rootfs_dir.clone();
                guard_root = &self.rootfs_dir;
            } else {
                host_path.push("global");
                guard_root = &self.root;
            }
        }
        host_path.push(virtual_root.host_prefix);
        for segment in &tail {
            host_path.push(segment);
        }
        ensure_inside_root(guard_root, &host_path)?;
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
        let paths = self.session_paths(session_id)?;
        for dir in [
            &paths.uploads,
            &paths.browser,
            &paths.mounts,
            &paths.offloads,
            &paths.workspace,
        ] {
            fs::create_dir_all(dir).map_err(|error| {
                HamburError::Internal(format!(
                    "create sandbox session dir {}: {error}",
                    dir.display()
                ))
            })?;
        }
        Ok(())
    }

    fn session_paths(&self, session_id: &str) -> HamburResult<SessionPaths> {
        let session_id = normalize_session_id(session_id)?;
        let session_dir = self.root.join("sessions").join(&session_id);
        let attachments = session_dir.join("attachments");
        let paths = SessionPaths {
            session_id,
            uploads: attachments.join("uploads"),
            browser: session_dir.join("browser"),
            mounts: session_dir.join("mounts"),
            offloads: session_dir.join("offloads"),
            workspace: session_dir.join("workspace"),
            attachments,
        };
        for dir in [
            &paths.attachments,
            &paths.uploads,
            &paths.browser,
            &paths.mounts,
            &paths.offloads,
            &paths.workspace,
        ] {
            ensure_inside_root(&self.root, dir)?;
        }
        Ok(paths)
    }

    fn prepared_session_paths(&self, session_id: &str) -> HamburResult<SessionPaths> {
        let paths = self.session_paths(session_id)?;
        for dir in [
            &paths.attachments,
            &paths.uploads,
            &paths.browser,
            &paths.mounts,
            &paths.offloads,
            &paths.workspace,
        ] {
            if !dir.is_dir() {
                return Err(HamburError::Internal(format!(
                    "sandbox session directory is missing: {}. Session directories must be prepared when the session is created.",
                    dir.display()
                )));
            }
        }
        Ok(paths)
    }

    pub fn is_rootfs_installed(&self) -> bool {
        let shell = self.rootfs_dir.join("bin/sh");
        let busybox = self.rootfs_dir.join("bin/busybox");
        let shell_meta = fs::symlink_metadata(&shell);
        let busybox_meta = fs::symlink_metadata(&busybox);
        let shell_exists = shell_meta.is_ok();
        let busybox_is_file = busybox_meta
            .as_ref()
            .map(|metadata| metadata.file_type().is_file())
            .unwrap_or(false);
        let installed = shell_exists && busybox_is_file;
        eprintln!(
            "RootfsDebug is_rootfs_installed installed={} rootfs_dir={} shell={} shell_ok={} shell_err={} busybox={} busybox_file={} busybox_err={}",
            installed,
            self.rootfs_dir.display(),
            shell.display(),
            shell_exists,
            shell_meta
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default(),
            busybox.display(),
            busybox_is_file,
            busybox_meta
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default()
        );
        installed
    }

    fn recreate_rootfs_dir_for_install(&self) -> HamburResult<()> {
        if self.rootfs_dir.exists() {
            if let Err(e) = fs::remove_dir_all(&self.rootfs_dir) {
                let trash_dir = self
                    .app_files_dir
                    .join(format!("alpine-rootfs.bad-{}", now_ms()));
                if fs::rename(&self.rootfs_dir, &trash_dir).is_err() {
                    let rootfs_str = self.rootfs_dir.to_string_lossy();
                    let uid = std::process::Command::new("id")
                        .arg("-u")
                        .output()
                        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                        .unwrap_or_else(|_| "1000".to_string());
                    let script = format!(
                        "rm -rf -- '{0}' && mkdir -p -- '{0}' && chown {1}:{1} '{0}' && chmod 700 '{0}'",
                        rootfs_str.replace('\'', "'\\''"),
                        uid
                    );
                    let output = std::process::Command::new("su")
                        .arg("-c")
                        .arg(&script)
                        .output();
                    if let Ok(out) = output {
                        if !out.status.success() || !self.rootfs_dir.is_dir() {
                            return Err(HamburError::Internal(format!(
                                "rootfs cleanup via su failed: {}",
                                String::from_utf8_lossy(&out.stderr)
                            )));
                        }
                    } else {
                        return Err(HamburError::Internal(format!(
                            "rootfs cleanup failed, and fallback su command failed to spawn: {e}"
                        )));
                    }
                }
            }
        }
        if !self.rootfs_dir.exists() {
            fs::create_dir_all(&self.rootfs_dir)
                .map_err(|e| HamburError::Internal(format!("create rootfs dir: {e}")))?;
        }
        Ok(())
    }

    fn extract_tar_archive(&self, archive_path: &Path) -> HamburResult<()> {
        let file = File::open(archive_path)
            .map_err(|e| HamburError::Internal(format!("open rootfs archive: {e}")))?;
        let tar = GzDecoder::new(file);
        let mut archive = Archive::new(tar);
        archive
            .unpack(&self.rootfs_dir)
            .map_err(|e| HamburError::Internal(format!("unpack rootfs: {e}")))?;
        Ok(())
    }

    pub fn ensure_rootfs_skeleton(&self) -> HamburResult<()> {
        let var_hambur = self.rootfs_dir.join("var/hambur");
        let dirs = [
            var_hambur.join("attachments/uploads"),
            var_hambur.join("browser"),
            var_hambur.join("memory"),
            var_hambur.join("mounts"),
            var_hambur.join("offloads"),
            var_hambur.join("shared"),
            var_hambur.join("skills"),
            var_hambur.join("workspace"),
            var_hambur.join("download"),
            self.rootfs_dir.join("storage/emulated/0/Download"),
            self.rootfs_dir.join("etc"),
            self.rootfs_dir.join("var/minis/autostart"),
            self.rootfs_dir.join("dev"),
            self.rootfs_dir.join("proc"),
            self.rootfs_dir.join("sys"),
            self.rootfs_dir.join("tmp"),
            self.rootfs_dir.join("root"),
        ];
        for dir in &dirs {
            fs::create_dir_all(dir).map_err(|e| {
                HamburError::Internal(format!("create rootfs skeleton dir {}: {e}", dir.display()))
            })?;
        }
        let resolv_conf = self.rootfs_dir.join("etc/resolv.conf");
        if !resolv_conf.exists() || fs::read_to_string(&resolv_conf).map(|s| s.trim().is_empty()).unwrap_or(true) {
            let _ = fs::write(&resolv_conf, "nameserver 1.1.1.1\nnameserver 8.8.8.8\nnameserver 114.114.114.114\n");
        }
        let global = self.root.join("global");
        for name in &["memory", "skills", "shared", "download"] {
            fs::create_dir_all(global.join(name))
                .map_err(|e| HamburError::Internal(format!("create global dir {}: {e}", name)))?;
        }
        Ok(())
    }

    pub fn reset_rootfs(&self, preserve_root: bool) -> HamburResult<()> {
        let rootfs_archive = self.download_rootfs_archive()?;
        let root_home = self.rootfs_dir.join("root");
        let backup_dir = self.app_files_dir.join("alpine-rootfs-root-backup");
        if preserve_root && root_home.exists() {
            let _ = fs::remove_dir_all(&backup_dir);
            let _ = copy_dir_all(&root_home, &backup_dir);
        }
        self.unmount_chroot_mounts();
        self.clear_chroot_mount_state();
        self.clear_probe_cache();
        self.clear_startup_tasks_state();
        let _ = fs::remove_dir_all(self.root.join("global"));
        self.recreate_rootfs_dir_for_install()?;
        self.extract_tar_archive(&rootfs_archive.file)?;
        self.ensure_rootfs_skeleton()?;
        if !self.is_rootfs_installed() {
            return Err(HamburError::Internal(
                "Rootfs extraction incomplete: missing bin/sh or bin/busybox".to_string(),
            ));
        }
        if preserve_root && backup_dir.exists() {
            let _ = fs::remove_dir_all(&root_home);
            let _ = copy_dir_all(&backup_dir, &root_home);
            let _ = fs::remove_dir_all(&backup_dir);
        }
        let version_file = self.rootfs_dir.join(".hambur-rootfs.version");
        fs::write(&version_file, &rootfs_archive.version_label)
            .map_err(|e| HamburError::Internal(format!("write version file: {e}")))?;
        Ok(())
    }

    pub fn ensure_initialized(
        &self,
        startup_tasks: &[StartupTask],
        tasks_enabled: bool,
        requested_backend: &str,
    ) -> HamburResult<()> {
        if !self.is_rootfs_installed() {
            self.reset_rootfs(false)?;
        }
        self.ensure_rootfs_skeleton()?;
        self.copy_fallback_proot_asset_if_present();
        self.update_rootfs_status(requested_backend);
        self.prewarm_chroot_if_available();
        self.run_startup_tasks_once(startup_tasks, tasks_enabled)?;
        Ok(())
    }

    pub fn run_startup_tasks_once(&self, tasks: &[StartupTask], enabled: bool) -> HamburResult<()> {
        let should_run = {
            let mut ran = self.startup_tasks_ran.lock().map_err(|_| {
                HamburError::Internal("startup task state lock poisoned".to_string())
            })?;
            if *ran {
                false
            } else {
                *ran = true;
                true
            }
        };
        if should_run {
            self.run_startup_tasks(tasks, enabled)?;
        }
        Ok(())
    }

    pub fn run_startup_tasks(&self, tasks: &[StartupTask], enabled: bool) -> HamburResult<()> {
        if !enabled || tasks.is_empty() {
            return Ok(());
        }
        let autostart_dir = self.rootfs_dir.join("var/minis/autostart");
        let _ = fs::create_dir_all(&autostart_dir);
        let expected_names: std::collections::HashSet<String> = tasks
            .iter()
            .map(|t| format!("hambur-{}.sh", t.id))
            .collect();
        if let Ok(entries) = fs::read_dir(&autostart_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with("hambur-")
                    && name.ends_with(".sh")
                    && !expected_names.contains(&name)
                {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        for task in tasks {
            let file_name = format!("hambur-{}.sh", task.id);
            let task_file = autostart_dir.join(&file_name);
            let mut script = task.script.replace("\r\n", "\n").replace('\r', "\n");
            if !script.ends_with('\n') {
                script.push('\n');
            }
            let _ = fs::write(&task_file, script);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if task.enabled {
                    let _ = fs::set_permissions(&task_file, fs::Permissions::from_mode(0o755));
                } else {
                    let _ = fs::set_permissions(&task_file, fs::Permissions::from_mode(0o644));
                }
            }
            if task.enabled {
                let _ = self.prepare_session("startup_tasks");
                let cmd = format!("/bin/sh /var/minis/autostart/{}", file_name);
                let _ = self.execute("startup_tasks", &cmd, "/", 120_000);
            }
        }
        Ok(())
    }

    pub fn probe_rootfs_status(&self, requested_backend: &str) -> RootfsStatus {
        let abi = android_abi();
        eprintln!(
            "RootfsDebug probe_rootfs_status_start requested_backend={} abi={} rootfs_dir={}",
            requested_backend,
            abi,
            self.rootfs_dir.display()
        );
        if abi != "arm64-v8a" {
            eprintln!(
                "RootfsDebug probe_rootfs_status_result available=false backend={} reason=UnsupportedAbi",
                normalize_backend(requested_backend)
            );
            return RootfsStatus {
                available: false,
                backend: normalize_backend(requested_backend),
                abi,
                reason: "UnsupportedAbi".to_string(),
            };
        }
        let rootfs_installed = self.is_rootfs_installed();
        if !rootfs_installed {
            eprintln!(
                "RootfsDebug probe_rootfs_status_result available=false backend={} reason=rootfs_not_initialized",
                normalize_backend(requested_backend)
            );
            return RootfsStatus {
                available: false,
                backend: normalize_backend(requested_backend),
                abi,
                reason: "rootfs is not initialized".to_string(),
            };
        }
        let backend = normalize_backend(requested_backend);
        let proot_available = if backend == "proot" {
            self.probe_proot_available()
        } else {
            false
        };
        let chroot_available = if backend == "chroot" {
            self.probe_chroot_available()
        } else {
            false
        };
        let fallback_proot_available = if backend == "chroot" && !chroot_available {
            self.probe_proot_available()
        } else {
            proot_available
        };
        eprintln!(
            "RootfsDebug probe_rootfs_status_probes backend={} chroot_available={} proot_available={} fallback_proot_available={}",
            backend, chroot_available, proot_available, fallback_proot_available
        );
        if backend == "chroot" {
            if chroot_available {
                RootfsStatus {
                    available: true,
                    backend: "chroot".to_string(),
                    abi,
                    reason: String::new(),
                }
            } else if fallback_proot_available {
                RootfsStatus {
                    available: true,
                    backend: "proot".to_string(),
                    abi,
                    reason: "chroot failed; fell back to proot".to_string(),
                }
            } else {
                RootfsStatus {
                    available: false,
                    backend: "none".to_string(),
                    abi,
                    reason: "both chroot and proot probes failed".to_string(),
                }
            }
        } else {
            if proot_available {
                RootfsStatus {
                    available: true,
                    backend: "proot".to_string(),
                    abi,
                    reason: String::new(),
                }
            } else {
                RootfsStatus {
                    available: false,
                    backend: "none".to_string(),
                    abi,
                    reason: "proot probe failed".to_string(),
                }
            }
        }
    }

    pub fn probe_root_available(&self) -> bool {
        if let Ok(cache) = self.probe_cache.lock() {
            if let Some(probe) = cache.root.as_ref().filter(|probe| probe.is_fresh()) {
                eprintln!(
                    "RootfsDebug probe_root_available cached=true available={}",
                    probe.available
                );
                return probe.available;
            }
        }
        let root_check = std::process::Command::new("su")
            .arg("-c")
            .arg("id -u")
            .output();
        let available = match root_check {
            Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "0",
                Err(_) => false,
            };
        eprintln!("RootfsDebug probe_root_available available={available}");
        if let Ok(mut cache) = self.probe_cache.lock() {
            cache.root = Some(TimedProbe {
                available,
                checked_at_ms: now_ms(),
            });
        }
        available
    }

    pub fn probe_chroot_available(&self) -> bool {
        if let Ok(cache) = self.probe_cache.lock() {
            if let Some(probe) = cache.chroot.as_ref().filter(|probe| probe.is_fresh()) {
                eprintln!(
                    "RootfsDebug probe_chroot_available cached=true available={}",
                    probe.available
                );
                return probe.available;
            }
        }
        let available = self.probe_root_available()
            && {
                let chroot_check = std::process::Command::new("su")
                .arg("-c")
                .arg(format!(
                    "chroot '{}' /bin/busybox --install -s /bin >/dev/null 2>&1; chroot '{}' /bin/sh -lc 'echo hambur-chroot-ok'",
                    self.rootfs_dir.to_string_lossy(),
                    self.rootfs_dir.to_string_lossy()
                ))
                .output();
                match chroot_check {
                    Ok(out) => {
                        eprintln!(
                            "RootfsDebug probe_chroot_available status={} stdout={} stderr={}",
                            out.status,
                            String::from_utf8_lossy(&out.stdout).trim(),
                            String::from_utf8_lossy(&out.stderr).trim()
                        );
                        out.status.success()
                            && String::from_utf8_lossy(&out.stdout).contains("hambur-chroot-ok")
                    }
                    Err(error) => {
                        eprintln!("RootfsDebug probe_chroot_available spawn_error={error}");
                        false
                    }
                }
            };
        eprintln!("RootfsDebug probe_chroot_available available={available}");
        if let Ok(mut cache) = self.probe_cache.lock() {
            cache.chroot = Some(TimedProbe {
                available,
                checked_at_ms: now_ms(),
            });
        }
        available
    }

    pub fn probe_proot_available(&self) -> bool {
        if let Ok(cache) = self.probe_cache.lock() {
            if let Some(probe) = cache.proot.as_ref().filter(|probe| probe.is_fresh()) {
                eprintln!(
                    "RootfsDebug probe_proot_available cached=true available={}",
                    probe.available
                );
                return probe.available;
            }
        }
        let available = if let Some(proot_bin) = self.find_proot_executable() {
            let tmp_dir = self.app_files_dir.join("tmp/proot");
            let _ = fs::create_dir_all(&tmp_dir);
            let tmp_dir_str = tmp_dir.to_string_lossy().into_owned();
            let proot_check = std::process::Command::new(&proot_bin)
                .arg("-0")
                .arg("-r")
                .arg(&self.rootfs_dir)
                .arg("-w")
                .arg("/")
                .arg("/bin/sh")
                .arg("-lc")
                .arg("echo hambur-proot-ok")
                .env("PROOT_TMP_DIR", &tmp_dir_str)
                .env("TMPDIR", &tmp_dir_str)
                .env("TEMP", &tmp_dir_str)
                .env("TMP", &tmp_dir_str)
                .output();
            match proot_check {
                Ok(out) => {
                    eprintln!(
                        "RootfsDebug probe_proot_available bin={} status={} stdout={} stderr={}",
                        proot_bin.display(),
                        out.status,
                        String::from_utf8_lossy(&out.stdout).trim(),
                        String::from_utf8_lossy(&out.stderr).trim()
                    );
                    out.status.success()
                        && String::from_utf8_lossy(&out.stdout).contains("hambur-proot-ok")
                }
                Err(error) => {
                    eprintln!(
                        "RootfsDebug probe_proot_available bin={} spawn_error={error}",
                        proot_bin.display()
                    );
                    false
                }
            }
        } else {
            eprintln!("RootfsDebug probe_proot_available missing_proot_binary=true");
            false
        };
        eprintln!("RootfsDebug probe_proot_available available={available}");
        if let Ok(mut cache) = self.probe_cache.lock() {
            cache.proot = Some(TimedProbe {
                available,
                checked_at_ms: now_ms(),
            });
        }
        available
    }

    pub fn update_rootfs_status(&self, requested_backend: &str) {
        let new_status = self.probe_rootfs_status(requested_backend);
        if let Ok(mut status) = self.rootfs_status.lock() {
            *status = new_status;
        }
    }

    pub fn prewarm_chroot_if_available(&self) {
        if self.rootfs_status().backend == "chroot" {
            let _ = self.prepare_chroot_static_mounts();
        }
    }

    fn find_proot_executable(&self) -> Option<PathBuf> {
        if let Some(native_library_dir) = &self.native_library_dir {
            let libproot = native_library_dir.join("libproot.so");
            if libproot.is_file() {
                return Some(libproot);
            }
        }
        if let Some(parent) = self.app_files_dir.parent() {
            let lib_dir = parent.join("lib");
            let libproot = lib_dir.join("libproot.so");
            if libproot.is_file() {
                return Some(libproot);
            }
        }
        let fallback = self.app_files_dir.join("bin/proot");
        if fallback.is_file() {
            return Some(fallback);
        }
        None
    }

    #[allow(dead_code)]
    fn find_host_executable(&self, name: &str) -> Option<String> {
        let candidates = [
            format!("/system/bin/{}", name),
            format!("/system/xbin/{}", name),
            format!("/vendor/bin/{}", name),
            format!("/data/adb/ksu/bin/{}", name),
            format!("/data/adb/magisk/{}", name),
        ];
        for path in &candidates {
            if Path::new(path).exists() {
                return Some(path.clone());
            }
        }
        None
    }

    #[allow(dead_code)]
    fn find_host_busybox_executable(&self) -> Option<PathBuf> {
        let candidates = [
            "/data/adb/ksu/bin/busybox",
            "/data/adb/magisk/busybox",
            "/system/xbin/busybox",
            "/system/bin/busybox",
        ];
        for path in &candidates {
            let p = Path::new(path);
            if p.exists() {
                return Some(p.to_path_buf());
            }
        }
        None
    }

    fn copy_fallback_proot_asset_if_present(&self) {}

    fn resolve_latest_rootfs_release(&self) -> HamburResult<(AlpineRootfsRelease, String)> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();
        let mut last_error = None;
        for &base_url in ROOTFS_RELEASES_BASE_URLS {
            let manifest_url = format!("{base_url}latest-releases.yaml");
            match client.get(&manifest_url).send() {
                Ok(resp) => {
                    if resp.status().is_success() {
                        if let Ok(text) = resp.text() {
                            if let Some(release) =
                                parse_alpine_manifest(&text, "aarch64", "alpine-minirootfs")
                            {
                                return Ok((release, base_url.to_string()));
                            }
                        }
                    } else {
                        last_error = Some(format!("HTTP {}", resp.status()));
                    }
                }
                Err(e) => {
                    last_error = Some(e.to_string());
                }
            }
        }
        Err(HamburError::Internal(format!(
            "Failed to resolve latest rootfs release: {}",
            last_error.unwrap_or_else(|| "no mirror responded".to_string())
        )))
    }

    fn download_rootfs_archive(&self) -> HamburResult<DownloadedRootfsArchive> {
        let cache_dir = self
            .app_files_dir
            .parent()
            .unwrap_or(&self.app_files_dir)
            .join("cache");
        let download_dir = cache_dir.join("hambur_rootfs_downloads");
        let _ = fs::create_dir_all(&download_dir);

        if std::env::var("HAMBUR_TEST_MOCK_ROOTFS").is_ok() {
            let mock_file = download_dir.join("alpine-minirootfs-mock-aarch64.tar.gz");
            if !mock_file.exists() {
                create_dummy_rootfs_tar_gz(&mock_file).map_err(|e| {
                    HamburError::Internal(format!("Failed to create dummy test rootfs: {e}"))
                })?;
            }
            let sha256 = calculate_sha256(&mock_file)?;
            let release = AlpineRootfsRelease {
                file_name: "alpine-minirootfs-mock-aarch64.tar.gz".to_string(),
                version: "mock-1.0".to_string(),
                sha256,
            };
            self.write_rootfs_cache_metadata(&download_dir, &release);
            return Ok(DownloadedRootfsArchive {
                file: mock_file,
                version_label: release.version,
            });
        }

        if let Some(cached) = self.load_verified_cached_rootfs_archive(&download_dir) {
            return Ok(cached);
        }
        let mut last_error = None;
        for attempt in 1..=3 {
            match self.download_rootfs_archive_once(&download_dir) {
                Ok(archive) => return Ok(archive),
                Err(e) => {
                    last_error = Some(e);
                    std::thread::sleep(std::time::Duration::from_millis(attempt * 1000));
                }
            }
        }
        Err(HamburError::Internal(format!(
            "Rootfs download failed: {}",
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        )))
    }

    fn download_rootfs_archive_once(
        &self,
        download_dir: &Path,
    ) -> HamburResult<DownloadedRootfsArchive> {
        let (release, base_url) = self.resolve_latest_rootfs_release()?;
        let download_url = format!("{}{}", base_url, release.file_name);
        let target = download_dir.join(&release.file_name);
        let partial = download_dir.join(format!("{}.part", release.file_name));
        let _ = self.cleanup_partial_downloads(download_dir);
        if target.exists() {
            if let Ok(sha) = calculate_sha256(&target) {
                if sha.to_lowercase() == release.sha256.to_lowercase()
                    && archive_contains_entries(&target)
                {
                    return Ok(DownloadedRootfsArchive {
                        file: target,
                        version_label: release.version.clone(),
                    });
                }
            }
        }
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();
        let mut resp = client
            .get(&download_url)
            .send()
            .map_err(|e| HamburError::Internal(format!("request rootfs archive: {e}")))?;
        if !resp.status().is_success() {
            return Err(HamburError::Internal(format!(
                "HTTP error downloading rootfs: {}",
                resp.status()
            )));
        }
        let mut out_file = File::create(&partial)
            .map_err(|e| HamburError::Internal(format!("create partial file: {e}")))?;
        resp.copy_to(&mut out_file)
            .map_err(|e| HamburError::Internal(format!("write partial file: {e}")))?;
        let sha = calculate_sha256(&partial)?;
        if sha.to_lowercase() != release.sha256.to_lowercase() {
            let _ = fs::remove_file(&partial);
            return Err(HamburError::Internal(format!(
                "sha256 mismatch: expected {}, got {}",
                release.sha256, sha
            )));
        }
        let _ = fs::remove_file(&target);
        fs::rename(&partial, &target)
            .map_err(|e| HamburError::Internal(format!("rename partial file: {e}")))?;
        self.write_rootfs_cache_metadata(download_dir, &release);
        Ok(DownloadedRootfsArchive {
            file: target,
            version_label: release.version,
        })
    }

    fn write_rootfs_cache_metadata(&self, download_dir: &Path, release: &AlpineRootfsRelease) {
        let meta_file = download_dir.join(".hambur-rootfs-download");
        let content = format!(
            "{}\n{}\n{}\n",
            release.file_name,
            release.sha256.to_lowercase(),
            release.version
        );
        let _ = fs::write(meta_file, content);
    }

    fn load_verified_cached_rootfs_archive(
        &self,
        download_dir: &Path,
    ) -> Option<DownloadedRootfsArchive> {
        let meta_file = download_dir.join(".hambur-rootfs-download");
        if let Ok(content) = fs::read_to_string(&meta_file) {
            let lines: Vec<&str> = content
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty())
                .collect();
            if lines.len() >= 3 {
                let file_name = lines[0];
                let sha256 = lines[1].to_lowercase();
                let version = lines[2];
                let target = download_dir.join(file_name);
                if target.exists() {
                    if let Ok(sha) = calculate_sha256(&target) {
                        if sha.to_lowercase() == sha256 && archive_contains_entries(&target) {
                            return Some(DownloadedRootfsArchive {
                                file: target,
                                version_label: version.to_string(),
                            });
                        }
                    }
                }
            }
        }
        None
    }

    fn cleanup_partial_downloads(&self, download_dir: &Path) -> HamburResult<()> {
        if let Ok(entries) = fs::read_dir(download_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && path.extension().map(|e| e == "part").unwrap_or(false) {
                    let _ = fs::remove_file(path);
                }
            }
        }
        Ok(())
    }

    pub fn execute(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        timeout_ms: u64,
    ) -> HamburResult<SandboxExecResult> {
        let status = self.rootfs_status();
        if status.backend == "chroot" {
            let result = self.execute_chroot(session_id, command, cwd, timeout_ms);
            match result {
                Ok(mut r) => {
                    let has_marker = r.stdout.contains("__HAMBUR_SANDBOX_READY__");
                    if has_marker {
                        if let Some(stripped) = r.stdout.strip_prefix("__HAMBUR_SANDBOX_READY__\n") {
                            r.stdout = stripped.to_string();
                        } else if let Some(stripped) = r.stdout.strip_prefix("__HAMBUR_SANDBOX_READY__\r\n") {
                            r.stdout = stripped.to_string();
                        } else {
                            r.stdout = r
                                .stdout
                                .replace("__HAMBUR_SANDBOX_READY__\n", "")
                                .replace("__HAMBUR_SANDBOX_READY__\r\n", "")
                                .replace("__HAMBUR_SANDBOX_READY__", "");
                        }
                    }
                    if has_marker || r.exit_code >= 0 || r.timed_out || !self.probe_proot_available() {
                        return Ok(r);
                    }
                    let mut pr = self.execute_proot(session_id, command, cwd, timeout_ms)?;
                    pr.fallback_from = Some("chroot".to_string());
                    pr.warning = Some(format!(
                        "chroot setup failed (exit={}); fell back to proot",
                        r.exit_code
                    ));
                    return Ok(pr);
                }
                Err(e) => {
                    if !self.probe_proot_available() {
                        return Err(e);
                    }
                    let mut pr = self.execute_proot(session_id, command, cwd, timeout_ms)?;
                    pr.fallback_from = Some("chroot".to_string());
                    pr.warning = Some(format!("chroot setup error: {e}; fell back to proot"));
                    return Ok(pr);
                }
            }
        }
        self.execute_proot(session_id, command, cwd, timeout_ms)
    }

    fn execute_chroot(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        timeout_ms: u64,
    ) -> HamburResult<SandboxExecResult> {
        let exec_lock = self.chroot_exec_lock.clone();
        let _guard = exec_lock
            .lock()
            .map_err(|_| HamburError::Internal("chroot execution lock poisoned".to_string()))?;
        let (program, args, _env) = self.build_chroot_command(session_id, command, cwd, true, timeout_ms)?;
        self.run_process_blocking(
            program,
            args,
            std::collections::HashMap::new(),
            timeout_ms,
            "chroot",
            session_id,
            cwd,
        )
    }

    fn execute_proot(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        timeout_ms: u64,
    ) -> HamburResult<SandboxExecResult> {
        let (program, args, env) = self.build_proot_command(session_id, command, cwd, false, timeout_ms)?;
        self.run_process_blocking(program, args, env, timeout_ms, "proot", session_id, cwd)
    }

    fn build_chroot_command(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        emit_ready_marker: bool,
        _timeout_ms: u64,
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let root = self.rootfs_dir.to_string_lossy().into_owned();
        let marker_line = if emit_ready_marker {
            "printf '__HAMBUR_SANDBOX_READY__\\n'\n"
        } else {
            ""
        };
        self.prepare_chroot_mounts(session_id)?;
        let inner_script = format!(
            "export HOME=/root\n\
             export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
             umask 000\n\
             {}\
             cd {} || exit 127\n\
             {}",
            marker_line,
            shell_quote(cwd),
            command
        );
        let script = format!(
            "set +e\n\
             ROOT={}\n\
             chroot \"$ROOT\" /bin/sh -lc {}\n\
             STATUS=$?\n\
             exit $STATUS",
            shell_quote(&root),
            shell_quote(&inner_script)
        );
        Ok((
            "su".to_string(),
            vec!["-c".to_string(), script],
            std::collections::HashMap::new(),
        ))
    }

    fn build_proot_command(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        emit_ready_marker: bool,
        _timeout_ms: u64,
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let session_paths = self.prepared_session_paths(session_id)?;
        let proot_bin = self
            .find_proot_executable()
            .ok_or_else(|| HamburError::Internal("proot binary is missing".to_string()))?;
        let global_memory = self.root.join("global/memory");
        let global_skills = self.root.join("global/skills");
        let global_shared = self.root.join("global/shared");
        let marker_line = if emit_ready_marker {
            "printf '__HAMBUR_SANDBOX_READY__\\n'\n"
        } else {
            ""
        };
        let inner_script = format!(
            "export HOME=/root\n\
             export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
             umask 000\n\
             {}\
             cd {} || exit 127\n\
             {}",
            marker_line,
            shell_quote(cwd),
            command
        );
        let mut args = vec![
            "-0".to_string(),
            "-r".to_string(),
            self.rootfs_dir.to_string_lossy().into_owned(),
            "-b".to_string(),
            "/dev:/dev".to_string(),
            "-b".to_string(),
            "/proc:/proc".to_string(),
            "-b".to_string(),
            "/sys:/sys".to_string(),
            "-b".to_string(),
            format!("{}:/var/hambur/memory", global_memory.to_string_lossy()),
            "-b".to_string(),
            format!("{}:/var/hambur/skills", global_skills.to_string_lossy()),
            "-b".to_string(),
            format!("{}:/var/hambur/shared", global_shared.to_string_lossy()),
            "-b".to_string(),
            format!(
                "{}:/var/hambur/attachments",
                session_paths.attachments.to_string_lossy()
            ),
            "-b".to_string(),
            format!(
                "{}:/var/hambur/browser",
                session_paths.browser.to_string_lossy()
            ),
            "-b".to_string(),
            format!(
                "{}:/var/hambur/mounts",
                session_paths.mounts.to_string_lossy()
            ),
            "-b".to_string(),
            format!(
                "{}:/var/hambur/offloads",
                session_paths.offloads.to_string_lossy()
            ),
            "-b".to_string(),
            format!(
                "{}:/var/hambur/workspace",
                session_paths.workspace.to_string_lossy()
            ),
        ];
        if Path::new("/storage/emulated/0/Download").is_dir() {
            args.push("-b".to_string());
            args.push("/storage/emulated/0/Download:/storage/emulated/0/Download".to_string());
            args.push("-b".to_string());
            args.push("/storage/emulated/0/Download:/var/hambur/download".to_string());
        }
        args.extend([
            "-w".to_string(),
            cwd.to_string(),
            "/bin/sh".to_string(),
            "-lc".to_string(),
            inner_script,
        ]);
        let tmp_dir = self.app_files_dir.join("tmp/proot");
        let _ = fs::create_dir_all(&tmp_dir);
        let tmp_dir_str = tmp_dir.to_string_lossy().into_owned();
        let mut env = std::collections::HashMap::new();
        env.insert("PROOT_TMP_DIR".to_string(), tmp_dir_str.clone());
        env.insert("TMPDIR".to_string(), tmp_dir_str.clone());
        env.insert("TEMP".to_string(), tmp_dir_str.clone());
        env.insert("TMP".to_string(), tmp_dir_str);
        Ok((proot_bin.to_string_lossy().into_owned(), args, env))
    }

    pub fn build_execution_command(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        process_session_id: &str,
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let status = self.rootfs_status();
        let backend = status.backend.as_str();
        if backend == "chroot" {
            self.build_chroot_background_command(session_id, command, cwd, process_session_id)
        } else {
            self.build_proot_command(session_id, command, cwd, false, 0)
        }
    }

    pub fn chroot_process_pid_file(&self, process_session_id: &str) -> HamburResult<PathBuf> {
        let process_session_id = normalize_session_id(process_session_id)?;
        let process_dir = self.root.join("processes");
        let pid_file = process_dir.join(format!("{process_session_id}.pid"));
        ensure_inside_root(&self.root, &pid_file)?;
        Ok(pid_file)
    }

    fn build_chroot_background_command(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        process_session_id: &str,
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let session_paths = self.prepared_session_paths(session_id)?;
        let pid_file = self.chroot_process_pid_file(process_session_id)?;
        if let Some(parent) = pid_file.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| HamburError::Internal(format!("create process dir: {e}")))?;
        }
        let root = self.rootfs_dir.to_string_lossy().into_owned();
        let global_memory = self.root.join("global/memory");
        let global_skills = self.root.join("global/skills");
        let global_shared = self.root.join("global/shared");
        for dir in [&global_memory, &global_skills, &global_shared] {
            fs::create_dir_all(dir).map_err(|e| {
                HamburError::Internal(format!("create chroot host dir {}: {e}", dir.display()))
            })?;
        }
        let inner_script = format!(
            "export HOME=/root\n\
             export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
             umask 000\n\
             cd {} || exit 127\n\
             {}",
            shell_quote(cwd),
            command
        );
        let script = format!(
            "set +e\n\
             ROOT={}\n\
             PIDFILE={}\n\
             echo $$ > \"$PIDFILE\"\n\
             umount_if_mounted() {{ grep -q \" $1 \" /proc/mounts 2>/dev/null && umount -l \"$1\" 2>/dev/null || true; }}\n\
             bind_dir() {{ mkdir -p \"$2\" && umount_if_mounted \"$2\" && mount -o bind \"$1\" \"$2\"; }}\n\
             cleanup() {{ STATUS=$?; if [ -n \"${{CHILD:-}}\" ]; then kill -TERM \"$CHILD\" 2>/dev/null || true; wait \"$CHILD\" 2>/dev/null || true; fi; for target in \"$ROOT/var/hambur/workspace\" \"$ROOT/var/hambur/offloads\" \"$ROOT/var/hambur/mounts\" \"$ROOT/var/hambur/browser\" \"$ROOT/var/hambur/attachments\" \"$ROOT/var/hambur/shared\" \"$ROOT/var/hambur/skills\" \"$ROOT/var/hambur/memory\" \"$ROOT/var/hambur/download\" \"$ROOT/storage/emulated/0/Download\" \"$ROOT/sys\" \"$ROOT/proc\" \"$ROOT/dev\"; do umount_if_mounted \"$target\"; done; rm -f \"$PIDFILE\"; exit $STATUS; }}\n\
             trap cleanup EXIT INT TERM\n\
             mount --make-rprivate / 2>/dev/null || true\n\
             umount_if_mounted \"$ROOT/dev\"\n\
             umount_if_mounted \"$ROOT/proc\"\n\
             umount_if_mounted \"$ROOT/sys\"\n\
             mount -o bind /dev \"$ROOT/dev\"\n\
             mount -t proc proc \"$ROOT/proc\" 2>/dev/null || mount -o bind /proc \"$ROOT/proc\"\n\
             mount -o bind /sys \"$ROOT/sys\" 2>/dev/null || true\n\
             bind_dir {} \"$ROOT/var/hambur/memory\"\n\
             bind_dir {} \"$ROOT/var/hambur/skills\"\n\
             bind_dir {} \"$ROOT/var/hambur/shared\"\n\
             if [ -d /storage/emulated/0/Download ]; then \
               bind_dir /storage/emulated/0/Download \"$ROOT/storage/emulated/0/Download\"; \
               bind_dir /storage/emulated/0/Download \"$ROOT/var/hambur/download\"; \
               ln -sf /storage/emulated/0/Download \"$ROOT/sdcard/Download\" 2>/dev/null || true; \
             fi\n\
             bind_dir {} \"$ROOT/var/hambur/attachments\"\n\
             bind_dir {} \"$ROOT/var/hambur/browser\"\n\
             bind_dir {} \"$ROOT/var/hambur/mounts\"\n\
             bind_dir {} \"$ROOT/var/hambur/offloads\"\n\
             bind_dir {} \"$ROOT/var/hambur/workspace\"\n\
             chroot \"$ROOT\" /bin/busybox --install -s /bin >/dev/null 2>&1 || true\n\
             chroot \"$ROOT\" /bin/sh -lc {} &\n\
             CHILD=$!\n\
             wait \"$CHILD\"\n\
             exit $?",
            shell_quote(&root),
            shell_quote(&pid_file.to_string_lossy()),
            shell_quote(&global_memory.to_string_lossy()),
            shell_quote(&global_skills.to_string_lossy()),
            shell_quote(&global_shared.to_string_lossy()),
            shell_quote(&session_paths.attachments.to_string_lossy()),
            shell_quote(&session_paths.browser.to_string_lossy()),
            shell_quote(&session_paths.mounts.to_string_lossy()),
            shell_quote(&session_paths.offloads.to_string_lossy()),
            shell_quote(&session_paths.workspace.to_string_lossy()),
            shell_quote(&inner_script)
        );
        let wrapper = format!("unshare -m /system/bin/sh -c {}", shell_quote(&script));
        Ok((
            "su".to_string(),
            vec!["-c".to_string(), wrapper],
            std::collections::HashMap::new(),
        ))
    }

    fn run_process_blocking(
        &self,
        program: String,
        args: Vec<String>,
        env: std::collections::HashMap<String, String>,
        timeout_ms: u64,
        backend: &str,
        session_id: &str,
        cwd: &str,
    ) -> HamburResult<SandboxExecResult> {
        let start_time = std::time::Instant::now();
        let mut cmd = std::process::Command::new(&program);
        cmd.args(&args);
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| HamburError::Internal(format!("spawn {program}: {e}")))?;

        let mut stdout_handle = child.stdout.take();
        let mut stderr_handle = child.stderr.take();

        let stdout_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut out) = stdout_handle.take() {
                let _ = out.read_to_end(&mut buf);
            }
            buf
        });

        let stderr_thread = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut err) = stderr_handle.take() {
                let _ = err.read_to_end(&mut buf);
            }
            buf
        });

        let deadline = if timeout_ms > 0 {
            Some(std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms + 1500))
        } else {
            None
        };
        let mut exit_code = -1;
        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_code = status.code().unwrap_or(-1);
                    break;
                }
                Ok(None) => {
                    if let Some(dl) = deadline {
                        if std::time::Instant::now() >= dl {
                            #[cfg(unix)]
                            {
                                let pid = child.id() as i32;
                                unsafe {
                                    libc::killpg(pid, libc::SIGKILL);
                                }
                                let _ = std::process::Command::new("su")
                                    .arg("-c")
                                    .arg(format!("kill -KILL -{pid} 2>/dev/null; pkill -KILL -s {pid} 2>/dev/null"))
                                    .output();
                            }
                            let _ = child.kill();
                            let _ = child.wait();
                            timed_out = true;
                            break;
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => {
                    #[cfg(unix)]
                    {
                        let pid = child.id() as i32;
                        unsafe {
                            libc::killpg(pid, libc::SIGKILL);
                        }
                        let _ = std::process::Command::new("su")
                            .arg("-c")
                            .arg(format!("kill -KILL -{pid} 2>/dev/null; pkill -KILL -s {pid} 2>/dev/null"))
                            .output();
                    }
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
        let stdout_bytes = stdout_thread.join().unwrap_or_default();
        let stderr_bytes = stderr_thread.join().unwrap_or_default();
        let stdout_str = String::from_utf8_lossy(&stdout_bytes).into_owned();
        let stderr_str = String::from_utf8_lossy(&stderr_bytes).into_owned();
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
        let timed_out = timed_out || exit_code == 137 || exit_code == 143;
        Ok(SandboxExecResult {
            backend: backend.to_string(),
            exit_code,
            stdout: stdout_str,
            stderr: stderr_str,
            timed_out,
            elapsed_ms,
            cwd: cwd.to_string(),
            session_id: session_id.to_string(),
            fallback_from: None,
            warning: None,
        })
    }

    pub fn unmount_chroot_mounts(&self) {
        let rootfs_str = self.rootfs_dir.to_string_lossy();
        let script = format!(
            "ROOT='{}'\n\
             umount_if_mounted() {{ grep -q \" $1 \" /proc/mounts 2>/dev/null && umount -l \"$1\" 2>/dev/null || true; }}\n\
             for target in \"$ROOT/var/hambur/workspace\" \"$ROOT/var/hambur/offloads\" \"$ROOT/var/hambur/mounts\" \"$ROOT/var/hambur/browser\" \"$ROOT/var/hambur/attachments\" \"$ROOT/var/hambur/shared\" \"$ROOT/var/hambur/skills\" \"$ROOT/var/hambur/memory\" \"$ROOT/var/hambur/download\" \"$ROOT/storage/emulated/0/Download\" \"$ROOT/sys\" \"$ROOT/proc\" \"$ROOT/dev\"; do umount_if_mounted \"$target\"; done",
            rootfs_str.replace('\'', "'\\''")
        );
        let _ = std::process::Command::new("su")
            .arg("-c")
            .arg(&script)
            .output();
        self.clear_chroot_mount_state();
    }

    fn clear_chroot_mount_state(&self) {
        if let Ok(mut state) = self.chroot_mount_state.lock() {
            state.static_ready = false;
            state.prepared_session_id = None;
        }
    }

    fn clear_probe_cache(&self) {
        if let Ok(mut cache) = self.probe_cache.lock() {
            *cache = ProbeCache::default();
        }
    }

    fn clear_startup_tasks_state(&self) {
        if let Ok(mut ran) = self.startup_tasks_ran.lock() {
            *ran = false;
        }
    }

    fn prepare_chroot_static_mounts(&self) -> HamburResult<()> {
        if self
            .chroot_mount_state
            .lock()
            .map(|state| state.static_ready)
            .unwrap_or(false)
        {
            return Ok(());
        }
        self.ensure_rootfs_skeleton()?;
        let global_memory = self.root.join("global/memory");
        let global_skills = self.root.join("global/skills");
        let global_shared = self.root.join("global/shared");
        for dir in [&global_memory, &global_skills, &global_shared] {
            fs::create_dir_all(dir).map_err(|e| {
                HamburError::Internal(format!("create chroot host dir {}: {e}", dir.display()))
            })?;
        }

        let root = self.rootfs_dir.to_string_lossy().into_owned();
        let script = format!(
            "set +e\n\
             ROOT={}\n\
             umount_if_mounted() {{ grep -q \" $1 \" /proc/mounts 2>/dev/null && umount -l \"$1\" 2>/dev/null || true; }}\n\
             bind_dir() {{ mkdir -p \"$2\" && umount_if_mounted \"$2\" && mount -o bind \"$1\" \"$2\"; }}\n\
             umount_if_mounted \"$ROOT/dev\"\n\
             umount_if_mounted \"$ROOT/proc\"\n\
             umount_if_mounted \"$ROOT/sys\"\n\
             mount -o bind /dev \"$ROOT/dev\"\n\
             mount -t proc proc \"$ROOT/proc\" 2>/dev/null || mount -o bind /proc \"$ROOT/proc\"\n\
             mount -o bind /sys \"$ROOT/sys\" 2>/dev/null || true\n\
             bind_dir {} \"$ROOT/var/hambur/memory\"\n\
             bind_dir {} \"$ROOT/var/hambur/skills\"\n\
             bind_dir {} \"$ROOT/var/hambur/shared\"\n\
             if [ -d /storage/emulated/0/Download ]; then \
               bind_dir /storage/emulated/0/Download \"$ROOT/storage/emulated/0/Download\"; \
               bind_dir /storage/emulated/0/Download \"$ROOT/var/hambur/download\"; \
               ln -sf /storage/emulated/0/Download \"$ROOT/sdcard/Download\" 2>/dev/null || true; \
             fi\n\
             chroot \"$ROOT\" /bin/busybox --install -s /bin >/dev/null 2>&1 || true",
            shell_quote(&root),
            shell_quote(&global_memory.to_string_lossy()),
            shell_quote(&global_skills.to_string_lossy()),
            shell_quote(&global_shared.to_string_lossy())
        );
        let out = std::process::Command::new("su")
            .arg("-c")
            .arg(script)
            .output()
            .map_err(|e| HamburError::Internal(format!("prepare chroot static mounts: {e}")))?;
        if !out.status.success() {
            self.clear_chroot_mount_state();
            return Err(HamburError::Internal(format!(
                "prepare chroot static mounts failed: {}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            )));
        }
        if let Ok(mut state) = self.chroot_mount_state.lock() {
            state.static_ready = true;
        }
        Ok(())
    }

    fn prepare_chroot_mounts(&self, session_id: &str) -> HamburResult<()> {
        let paths = self.prepared_session_paths(session_id)?;
        self.prepare_chroot_static_mounts()?;
        if self
            .chroot_mount_state
            .lock()
            .map(|state| state.prepared_session_id.as_deref() == Some(&paths.session_id))
            .unwrap_or(false)
        {
            return Ok(());
        }

        let root = self.rootfs_dir.to_string_lossy().into_owned();
        let script = format!(
            "set +e\n\
             ROOT={}\n\
             umount_if_mounted() {{ grep -q \" $1 \" /proc/mounts 2>/dev/null && umount -l \"$1\" 2>/dev/null || true; }}\n\
             bind_dir() {{ mkdir -p \"$2\" && umount_if_mounted \"$2\" && mount -o bind \"$1\" \"$2\"; }}\n\
             bind_dir {} \"$ROOT/var/hambur/attachments\"\n\
             bind_dir {} \"$ROOT/var/hambur/browser\"\n\
             bind_dir {} \"$ROOT/var/hambur/mounts\"\n\
             bind_dir {} \"$ROOT/var/hambur/offloads\"\n\
             bind_dir {} \"$ROOT/var/hambur/workspace\"",
            shell_quote(&root),
            shell_quote(&paths.attachments.to_string_lossy()),
            shell_quote(&paths.browser.to_string_lossy()),
            shell_quote(&paths.mounts.to_string_lossy()),
            shell_quote(&paths.offloads.to_string_lossy()),
            shell_quote(&paths.workspace.to_string_lossy())
        );
        let out = std::process::Command::new("su")
            .arg("-c")
            .arg(script)
            .output()
            .map_err(|e| HamburError::Internal(format!("prepare chroot session mounts: {e}")))?;
        if !out.status.success() {
            if let Ok(mut state) = self.chroot_mount_state.lock() {
                state.prepared_session_id = None;
            }
            return Err(HamburError::Internal(format!(
                "prepare chroot session mounts failed: {}{}",
                String::from_utf8_lossy(&out.stderr),
                String::from_utf8_lossy(&out.stdout)
            )));
        }
        if let Ok(mut state) = self.chroot_mount_state.lock() {
            state.prepared_session_id = Some(paths.session_id);
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

fn normalize_sandbox_path(raw_path: &str) -> HamburResult<String> {
    let mut path = raw_path.trim();
    if let Some(stripped) = path.strip_prefix("file://") {
        path = stripped;
    } else if let Some(stripped) = path.strip_prefix("hambur://") {
        path = stripped;
    } else if let Some(stripped) = path.strip_prefix("hambur:") {
        path = stripped;
    }
    let path = path.trim();
    if path.is_empty() {
        return Err(HamburError::InvalidCommand(
            "sandbox path must not be empty".to_string(),
        ));
    }

    let owned_path: String;
    let path = if path.starts_with('/') {
        path
    } else if path.starts_with("var/hambur/") {
        owned_path = format!("/{path}");
        &owned_path
    } else {
        return Err(HamburError::InvalidCommand(format!(
            "invalid sandbox path: '{raw_path}'. All tools strictly require an absolute sandbox path starting with {HAMBUR_PREFIX}/ (e.g. {HAMBUR_PREFIX}/workspace/...)"
        )));
    };

    if !(path == HAMBUR_PREFIX
        || path.starts_with(&format!("{HAMBUR_PREFIX}/"))
        || path == AUTOSTART_PREFIX
        || path.starts_with(&format!("{AUTOSTART_PREFIX}/")))
    {
        return Err(HamburError::InvalidCommand(format!(
            "invalid sandbox path: '{raw_path}'. All tools strictly require an absolute sandbox path starting with {HAMBUR_PREFIX}/ (e.g. {HAMBUR_PREFIX}/workspace/...)"
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

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn parse_alpine_manifest(manifest: &str, arch: &str, flavor: &str) -> Option<AlpineRootfsRelease> {
    let mut records = Vec::new();
    let mut current = std::collections::HashMap::new();

    let mut finish_record = |curr: &mut std::collections::HashMap<String, String>| {
        if !curr.is_empty() {
            records.push(curr.clone());
            curr.clear();
        }
    };

    for raw_line in manifest.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line == "---" {
            continue;
        }
        if line == "-" {
            finish_record(&mut current);
            continue;
        }
        let item_line = if line.starts_with("- ") {
            finish_record(&mut current);
            line[2..].trim()
        } else {
            line
        };
        if let Some(pos) = item_line.find(':') {
            let key = item_line[..pos].trim().to_string();
            let mut value = item_line[pos + 1..].trim().to_string();
            if value == "|" {
                continue;
            }
            if (value.starts_with('"') && value.ends_with('"'))
                || (value.starts_with('\'') && value.ends_with('\''))
            {
                if value.len() >= 2 {
                    value = value[1..value.len() - 1].to_string();
                }
            }
            current.insert(key, value);
        }
    }
    finish_record(&mut current);

    for record in records {
        if record.get("flavor").map(|s| s.as_str()) != Some(flavor)
            || record.get("arch").map(|s| s.as_str()) != Some(arch)
        {
            continue;
        }
        let file_name = record.get("file").cloned().unwrap_or_default();
        let sha256 = record.get("sha256").cloned().unwrap_or_default();
        let version = record.get("version").cloned().unwrap_or_default();

        if file_name.contains('/') || file_name.contains('\\') {
            continue;
        }
        if !file_name.starts_with(flavor) || !file_name.ends_with(".tar.gz") {
            continue;
        }
        if sha256.len() != 64 || !sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }

        return Some(AlpineRootfsRelease {
            file_name,
            version,
            sha256: sha256.to_lowercase(),
        });
    }
    None
}

fn archive_contains_entries(path: &Path) -> bool {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let tar = GzDecoder::new(file);
    let mut archive = Archive::new(tar);
    let entries = match archive.entries() {
        Ok(e) => e,
        Err(_) => return false,
    };

    let mut has_shell = false;
    let mut has_busybox = false;
    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = match entry.path() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let path_str = path.to_string_lossy().replace('\\', "/");
        let clean_path = path_str.trim_start_matches("./").trim_start_matches('/');
        if clean_path == "bin/sh" {
            has_shell = true;
        }
        if clean_path == "bin/busybox" {
            has_busybox = true;
        }
        if has_shell && has_busybox {
            return true;
        }
    }
    false
}

fn calculate_sha256(path: &Path) -> HamburResult<String> {
    let mut file = File::open(path)
        .map_err(|e| HamburError::Internal(format!("open file for sha256: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|e| HamburError::Internal(format!("read file for sha256: {e}")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &dst.join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), dst.join(entry.file_name()))?;
        }
    }
    Ok(())
}

fn create_dummy_rootfs_tar_gz(path: &Path) -> std::io::Result<()> {
    let file = File::create(path)?;
    let enc = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(enc);

    // Add dummy bin/sh and bin/busybox
    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o755);

    tar.append_data(&mut header, "bin/sh", &[][..])?;
    tar.append_data(&mut header, "bin/busybox", &[][..])?;

    tar.finish()?;
    Ok(())
}
