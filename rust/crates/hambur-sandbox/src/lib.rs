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
        Ok(Self {
            root,
            app_files_dir,
            native_library_dir,
            rootfs_dir,
            rootfs_status: Arc::new(Mutex::new(RootfsStatus::for_target("proot"))),
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

    pub fn is_rootfs_installed(&self) -> bool {
        let shell = self.rootfs_dir.join("bin/sh");
        let busybox = self.rootfs_dir.join("bin/busybox");
        fs::symlink_metadata(&shell).is_ok()
            && fs::symlink_metadata(&busybox)
                .map(|metadata| metadata.file_type().is_file())
                .unwrap_or(false)
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
        let global = self.root.join("global");
        for name in &["memory", "skills", "shared", "autostart"] {
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
        let _ = fs::remove_dir_all(self.root.join("sessions"));
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
    ) -> HamburResult<()> {
        if !self.is_rootfs_installed() {
            self.reset_rootfs(false)?;
        }
        self.ensure_rootfs_skeleton()?;
        self.copy_fallback_proot_asset_if_present();
        self.run_startup_tasks(startup_tasks, tasks_enabled)?;
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
                let cmd = format!("/bin/sh /var/minis/autostart/{}", file_name);
                let _ = self.execute("startup_tasks", &cmd, "/", 120_000);
            }
        }
        Ok(())
    }

    pub fn probe_rootfs_status(&self, requested_backend: &str) -> RootfsStatus {
        let abi = android_abi();
        if abi != "arm64-v8a" {
            return RootfsStatus {
                available: false,
                backend: normalize_backend(requested_backend),
                abi,
                reason: "UnsupportedAbi".to_string(),
            };
        }
        let rootfs_installed = self.is_rootfs_installed();
        if !rootfs_installed {
            return RootfsStatus {
                available: false,
                backend: normalize_backend(requested_backend),
                abi,
                reason: "rootfs is not initialized".to_string(),
            };
        }
        let root_check = std::process::Command::new("su")
            .arg("-c")
            .arg("id -u")
            .output();
        let root_available = match root_check {
            Ok(out) => out.status.success() && String::from_utf8_lossy(&out.stdout).trim() == "0",
            Err(_) => false,
        };
        let mut chroot_available = false;
        if root_available {
            let chroot_check = std::process::Command::new("su")
                .arg("-c")
                .arg(format!(
                    "chroot '{}' /bin/busybox --install -s /bin >/dev/null 2>&1; chroot '{}' /bin/sh -lc 'echo hambur-chroot-ok'",
                    self.rootfs_dir.to_string_lossy(),
                    self.rootfs_dir.to_string_lossy()
                ))
                .output();
            if let Ok(out) = chroot_check {
                chroot_available = out.status.success()
                    && String::from_utf8_lossy(&out.stdout).contains("hambur-chroot-ok");
            }
        }
        let mut proot_available = false;
        if let Some(proot_bin) = self.find_proot_executable() {
            let tmp_dir = self.app_files_dir.join("tmp/proot");
            let _ = fs::create_dir_all(&tmp_dir);
            let tmp_dir_str = tmp_dir.to_string_lossy().into_owned();
            let proot_check = std::process::Command::new(proot_bin)
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
            if let Ok(out) = proot_check {
                proot_available = out.status.success()
                    && String::from_utf8_lossy(&out.stdout).contains("hambur-proot-ok");
            }
        }
        let backend = normalize_backend(requested_backend);
        if backend == "chroot" {
            if chroot_available {
                RootfsStatus {
                    available: true,
                    backend: "chroot".to_string(),
                    abi,
                    reason: String::new(),
                }
            } else if proot_available {
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

    pub fn update_rootfs_status(&self, requested_backend: &str) {
        let new_status = self.probe_rootfs_status(requested_backend);
        if let Ok(mut status) = self.rootfs_status.lock() {
            *status = new_status;
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
                Ok(ref r) if r.stdout.contains("__HAMBUR_SANDBOX_READY__") => {
                    let mut clean_r = r.clone();
                    clean_r.stdout = r
                        .stdout
                        .replace("__HAMBUR_SANDBOX_READY__\n", "")
                        .replace("__HAMBUR_SANDBOX_READY__\r\n", "")
                        .replace("__HAMBUR_SANDBOX_READY__", "")
                        .trim()
                        .to_string();
                    return Ok(clean_r);
                }
                _ => {
                    let mut r = self.execute_proot(session_id, command, cwd, timeout_ms)?;
                    r.fallback_from = Some("chroot".to_string());
                    r.warning = Some(match result {
                        Ok(ref res) => format!(
                            "chroot setup failed (exit={}); fell back to proot",
                            res.exit_code
                        ),
                        Err(e) => format!("chroot setup error: {}; fell back to proot", e),
                    });
                    return Ok(r);
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
        let (program, args, _env) = self.build_chroot_command(session_id, command, cwd, true)?;
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
        let (program, args, env) = self.build_proot_command(session_id, command, cwd, false)?;
        self.run_process_blocking(program, args, env, timeout_ms, "proot", session_id, cwd)
    }

    fn build_chroot_command(
        &self,
        session_id: &str,
        command: &str,
        cwd: &str,
        emit_ready_marker: bool,
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let root = self.rootfs_dir.to_string_lossy().into_owned();
        let global_memory = self
            .root
            .join("global/memory")
            .to_string_lossy()
            .into_owned();
        let global_skills = self
            .root
            .join("global/skills")
            .to_string_lossy()
            .into_owned();
        let global_shared = self
            .root
            .join("global/shared")
            .to_string_lossy()
            .into_owned();
        let workspace = self
            .resolve(session_id, "/var/hambur/workspace", SandboxAccess::Read)?
            .host_path
            .to_string_lossy()
            .into_owned();
        let attachments = self
            .resolve(session_id, "/var/hambur/attachments", SandboxAccess::Read)?
            .host_path
            .to_string_lossy()
            .into_owned();
        let browser = self
            .resolve(session_id, "/var/hambur/browser", SandboxAccess::Read)?
            .host_path
            .to_string_lossy()
            .into_owned();
        let mounts = self
            .resolve(session_id, "/var/hambur/mounts", SandboxAccess::Read)?
            .host_path
            .to_string_lossy()
            .into_owned();
        let offloads = self
            .resolve(session_id, "/var/hambur/offloads", SandboxAccess::Read)?
            .host_path
            .to_string_lossy()
            .into_owned();
        let marker_line = if emit_ready_marker {
            "printf '__HAMBUR_SANDBOX_READY__\\n'\n"
        } else {
            ""
        };
        let inner_script = format!(
            "export HOME=/root\n\
             export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
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
             umount_if_mounted() {{ grep -q \" $1 \" /proc/mounts 2>/dev/null && umount -l \"$1\" 2>/dev/null || true; }}\n\
             bind_dir() {{ mkdir -p \"$2\" && umount_if_mounted \"$2\" && mount -o bind \"$1\" \"$2\"; }}\n\
             grep -q \" $ROOT/dev \" /proc/mounts 2>/dev/null || mount -o bind /dev \"$ROOT/dev\"\n\
             grep -q \" $ROOT/proc \" /proc/mounts 2>/dev/null || mount -t proc proc \"$ROOT/proc\" 2>/dev/null || mount -o bind /proc \"$ROOT/proc\"\n\
             grep -q \" $ROOT/sys \" /proc/mounts 2>/dev/null || mount -o bind /sys \"$ROOT/sys\" 2>/dev/null || true\n\
             bind_dir {} \"$ROOT/var/hambur/memory\"\n\
             bind_dir {} \"$ROOT/var/hambur/skills\"\n\
             bind_dir {} \"$ROOT/var/hambur/shared\"\n\
             bind_dir {} \"$ROOT/var/hambur/attachments\"\n\
             bind_dir {} \"$ROOT/var/hambur/browser\"\n\
             bind_dir {} \"$ROOT/var/hambur/mounts\"\n\
             bind_dir {} \"$ROOT/var/hambur/offloads\"\n\
             bind_dir {} \"$ROOT/var/hambur/workspace\"\n\
             chroot \"$ROOT\" /bin/busybox --install -s /bin >/dev/null 2>&1 || true\n\
             chroot \"$ROOT\" /bin/sh -lc {}\n\
             STATUS=$?\n\
             exit $STATUS",
            shell_quote(&root),
            shell_quote(&global_memory),
            shell_quote(&global_skills),
            shell_quote(&global_shared),
            shell_quote(&attachments),
            shell_quote(&browser),
            shell_quote(&mounts),
            shell_quote(&offloads),
            shell_quote(&workspace),
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
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let proot_bin = self
            .find_proot_executable()
            .ok_or_else(|| HamburError::Internal("proot binary is missing".to_string()))?;
        let global_memory = self.root.join("global/memory");
        let global_skills = self.root.join("global/skills");
        let global_shared = self.root.join("global/shared");
        let workspace = self
            .resolve(session_id, "/var/hambur/workspace", SandboxAccess::Read)?
            .host_path;
        let attachments = self
            .resolve(session_id, "/var/hambur/attachments", SandboxAccess::Read)?
            .host_path;
        let browser = self
            .resolve(session_id, "/var/hambur/browser", SandboxAccess::Read)?
            .host_path;
        let mounts = self
            .resolve(session_id, "/var/hambur/mounts", SandboxAccess::Read)?
            .host_path;
        let offloads = self
            .resolve(session_id, "/var/hambur/offloads", SandboxAccess::Read)?
            .host_path;
        let marker_line = if emit_ready_marker {
            "printf '__HAMBUR_SANDBOX_READY__\\n'\n"
        } else {
            ""
        };
        let inner_script = format!(
            "export HOME=/root\n\
             export PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin\n\
             {}\
             cd {}\n\
             {}",
            marker_line,
            shell_quote(cwd),
            command
        );
        let args = vec![
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
            format!("{}:/var/hambur/attachments", attachments.to_string_lossy()),
            "-b".to_string(),
            format!("{}:/var/hambur/browser", browser.to_string_lossy()),
            "-b".to_string(),
            format!("{}:/var/hambur/mounts", mounts.to_string_lossy()),
            "-b".to_string(),
            format!("{}:/var/hambur/offloads", offloads.to_string_lossy()),
            "-b".to_string(),
            format!("{}:/var/hambur/workspace", workspace.to_string_lossy()),
            "-w".to_string(),
            cwd.to_string(),
            "/bin/sh".to_string(),
            "-lc".to_string(),
            inner_script,
        ];
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
    ) -> HamburResult<(
        String,
        Vec<String>,
        std::collections::HashMap<String, String>,
    )> {
        let status = self.rootfs_status();
        let backend = status.backend.as_str();
        if backend == "chroot" {
            self.build_chroot_command(session_id, command, cwd, false)
        } else {
            self.build_proot_command(session_id, command, cwd, false)
        }
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
        let mut child = cmd
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| HamburError::Internal(format!("spawn {program}: {e}")))?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        let mut exit_code = -1;
        let mut timed_out = false;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_code = status.code().unwrap_or(-1);
                    break;
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break;
                }
            }
        }
        let mut stdout_str = String::new();
        let mut stderr_str = String::new();
        if let Some(mut out) = child.stdout.take() {
            let _ = out.read_to_string(&mut stdout_str);
        }
        if let Some(mut err) = child.stderr.take() {
            let _ = err.read_to_string(&mut stderr_str);
        }
        let elapsed_ms = start_time.elapsed().as_millis() as u64;
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
             for target in \"$ROOT/var/hambur/workspace\" \"$ROOT/var/hambur/offloads\" \"$ROOT/var/hambur/mounts\" \"$ROOT/var/hambur/browser\" \"$ROOT/var/hambur/attachments\" \"$ROOT/var/hambur/shared\" \"$ROOT/var/hambur/skills\" \"$ROOT/var/hambur/memory\" \"$ROOT/sys\" \"$ROOT/proc\" \"$ROOT/dev\"; do umount_if_mounted \"$target\"; done",
            rootfs_str.replace('\'', "'\\''")
        );
        let _ = std::process::Command::new("su")
            .arg("-c")
            .arg(&script)
            .output();
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
