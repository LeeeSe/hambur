use std::fs;
use std::path::{Component, Path, PathBuf};

use hambur_core::{HamburError, HamburResult, new_id};

#[derive(Debug, Clone)]
pub struct FileStore {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFilePath {
    pub file_id: String,
    pub relative_path: String,
    pub host_path: PathBuf,
    pub sandbox_path: String,
}

impl FileStore {
    pub fn new(app_files_dir: impl AsRef<Path>) -> HamburResult<Self> {
        let root = app_files_dir.as_ref().join("filestore");
        fs::create_dir_all(&root)
            .map_err(|error| HamburError::Internal(format!("create file store: {error}")))?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn reserve_session_attachment(
        &self,
        session_id: &str,
        display_name: &str,
    ) -> HamburResult<StoredFilePath> {
        let session_id = safe_segment(session_id, "session_id")?;
        let file_id = new_id("file");
        let name = safe_file_name(display_name);
        let file_name = format!("{file_id}-{name}");
        let relative_path = format!("sessions/{session_id}/attachments/uploads/{file_name}");
        let sandbox_path = format!("/var/hambur/attachments/uploads/{file_name}");
        self.resolve_reserved(&file_id, &relative_path, sandbox_path)
    }

    pub fn reserve_cache_image(
        &self,
        session_id: &str,
        extension: &str,
    ) -> HamburResult<StoredFilePath> {
        let session_id = safe_segment(session_id, "session_id")?;
        let file_id = new_id("file");
        let extension = safe_extension(extension);
        let relative_path = format!("cache/{session_id}/{file_id}.{extension}");
        self.resolve_reserved(&file_id, &relative_path, format!("/var/hambur/{relative_path}"))
    }

    pub fn host_path_for_relative(&self, relative_path: &str) -> HamburResult<PathBuf> {
        let normalized = normalize_relative_path(relative_path)?;
        let candidate = self.root.join(&normalized);
        ensure_inside_root(&self.root, &candidate)?;
        Ok(candidate)
    }

    pub fn delete_relative_if_exists(&self, relative_path: &str) -> HamburResult<()> {
        let path = self.host_path_for_relative(relative_path)?;
        if path.exists() {
            fs::remove_file(&path).map_err(|error| {
                HamburError::Internal(format!(
                    "delete file store path {}: {error}",
                    path.display()
                ))
            })?;
        }
        prune_empty_parents(&self.root, path.parent());
        Ok(())
    }

    fn resolve_reserved(
        &self,
        file_id: &str,
        relative_path: &str,
        sandbox_path: String,
    ) -> HamburResult<StoredFilePath> {
        let host_path = self.host_path_for_relative(relative_path)?;
        if let Some(parent) = host_path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                HamburError::Internal(format!("create file store parent: {error}"))
            })?;
        }
        Ok(StoredFilePath {
            file_id: file_id.to_string(),
            relative_path: relative_path.to_string(),
            host_path,
            sandbox_path,
        })
    }
}

fn safe_segment(value: &str, label: &str) -> HamburResult<String> {
    let value = value.trim();
    if value.is_empty()
        || value.contains('/')
        || value.contains('\\')
        || value == "."
        || value == ".."
    {
        return Err(HamburError::InvalidCommand(format!(
            "invalid file store {label}"
        )));
    }
    Ok(value.chars().take(160).collect())
}

fn safe_file_name(name: &str) -> String {
    let trimmed = name.trim();
    let fallback = "attachment";
    let source = if trimmed.is_empty() {
        fallback
    } else {
        trimmed
    };
    let normalized = source
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('.')
        .chars()
        .take(96)
        .collect::<String>();
    if normalized.is_empty() {
        fallback.to_string()
    } else {
        normalized
    }
}

fn safe_extension(extension: &str) -> String {
    let extension = extension
        .trim()
        .trim_start_matches('.')
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>();
    if extension.is_empty() {
        "bin".to_string()
    } else {
        extension
    }
}

fn normalize_relative_path(relative_path: &str) -> HamburResult<String> {
    let path = Path::new(relative_path);
    if path.is_absolute() {
        return Err(HamburError::InvalidCommand(
            "file store relative path must not be absolute".to_string(),
        ));
    }

    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if part.is_empty() {
                    continue;
                }
                parts.push(part.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(HamburError::InvalidCommand(
                    "file store path must not escape root".to_string(),
                ));
            }
        }
    }

    if parts.is_empty() {
        return Err(HamburError::InvalidCommand(
            "file store path must not be empty".to_string(),
        ));
    }
    Ok(parts.join("/"))
}

fn ensure_inside_root(root: &Path, candidate: &Path) -> HamburResult<()> {
    let original_root = root.to_path_buf();
    let canonical_root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root)?;
            root.canonicalize()
        })
        .map_err(|error| HamburError::Internal(format!("canonicalize file store root: {error}")))?;
    let parent = candidate.parent().unwrap_or(candidate);
    let inside_root = match parent.canonicalize() {
        Ok(parent) => parent.starts_with(&canonical_root),
        Err(_) => candidate.starts_with(&original_root) || candidate.starts_with(&canonical_root),
    };
    if !inside_root {
        return Err(HamburError::InvalidCommand(
            "file store path escaped root".to_string(),
        ));
    }
    Ok(())
}

fn prune_empty_parents(root: &Path, parent: Option<&Path>) {
    let Some(parent) = parent else {
        return;
    };
    if parent == root {
        return;
    }
    if fs::remove_dir(parent).is_ok() {
        prune_empty_parents(root, parent.parent());
    }
}
