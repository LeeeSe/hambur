use crate::*;

pub(crate) fn safe_join(root: &Path, relative: &str) -> HamburResult<PathBuf> {
    if relative.contains('\0')
        || relative.contains("..")
        || relative.contains('\\')
        || Path::new(relative).is_absolute()
    {
        return Err(HamburError::InvalidCommand(
            "path must not escape root".to_string(),
        ));
    }
    let root = root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(root)?;
            root.canonicalize()
        })
        .map_err(|error| HamburError::Internal(format!("canonicalize root: {error}")))?;
    let candidate = root.join(relative);
    let check_path = if candidate.exists() {
        candidate.canonicalize().map_err(|error| {
            HamburError::Internal(format!(
                "canonicalize path {}: {error}",
                candidate.display()
            ))
        })?
    } else {
        candidate
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .unwrap_or_else(|| root.clone())
    };
    if !check_path.starts_with(&root) {
        return Err(HamburError::InvalidCommand("path escaped root".to_string()));
    }
    Ok(candidate)
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> HamburResult<String> {
    let root = root
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize root: {error}")))?;
    let path = path
        .canonicalize()
        .map_err(|error| HamburError::Internal(format!("canonicalize path: {error}")))?;
    if !path.starts_with(&root) {
        return Err(HamburError::InvalidCommand("path escaped root".to_string()));
    }
    Ok(path
        .strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .replace('\\', "/"))
}

pub(crate) fn collect_named_files(
    root: &Path,
    name: &str,
    output: &mut Vec<PathBuf>,
) -> HamburResult<()> {
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(root)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_named_files(&path, name, output)?;
        } else if path.file_name().and_then(|value| value.to_str()) == Some(name) {
            output.push(path);
        }
    }
    Ok(())
}

pub(crate) fn list_relative_files(root: &Path) -> HamburResult<Vec<String>> {
    let mut output = Vec::new();
    collect_relative_files(root, root, &mut output)?;
    output.sort();
    Ok(output)
}

pub(crate) fn collect_relative_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<String>,
) -> HamburResult<()> {
    if !current.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(current)
        .map_err(|error| HamburError::Internal(format!("read directory: {error}")))?
    {
        let entry = entry
            .map_err(|error| HamburError::Internal(format!("read directory entry: {error}")))?;
        let path = entry.path();
        if path.is_dir() {
            collect_relative_files(root, &path, output)?;
        } else if path.is_file() {
            output.push(
                path.strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        }
    }
    Ok(())
}

pub(crate) fn system_time_to_ms(value: std::time::SystemTime) -> Option<u64> {
    value
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

pub(crate) fn normalize_tool_sandbox_path(path: &str) -> String {
    let mut trimmed = path.trim();
    if let Some(stripped) = trimmed.strip_prefix("file://") {
        trimmed = stripped;
    } else if let Some(stripped) = trimmed.strip_prefix("hambur://") {
        trimmed = stripped;
    } else if let Some(stripped) = trimmed.strip_prefix("hambur:") {
        trimmed = stripped;
    }
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        "/var/hambur/workspace".to_string()
    } else if trimmed.starts_with('/') {
        trimmed.to_string()
    } else if trimmed.starts_with("var/hambur/") {
        format!("/{trimmed}")
    } else {
        trimmed.to_string()
    }
}

pub(crate) fn detect_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        Some("bmp") => "image/bmp",
        Some("svg") => "image/svg+xml",
        Some("mp3") => "audio/mpeg",
        Some("m4a") => "audio/mp4",
        Some("wav") => "audio/wav",
        Some("ogg") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mkv") => "video/x-matroska",
        Some("txt") | Some("log") => "text/plain",
        Some("md") | Some("markdown") => "text/markdown",
        Some("json") | Some("jsonl") => "application/json",
        Some("html") | Some("htm") => "text/html",
        Some("css") => "text/css",
        Some("js") => "application/javascript",
        Some("ts") => "application/typescript",
        Some("py") => "text/x-python",
        Some("sh") => "text/x-shellscript",
        Some("rs") => "text/rust",
        Some("pdf") => "application/pdf",
        _ => "application/octet-stream",
    }
}

pub(crate) fn collect_all_files(root: &Path, output: &mut Vec<PathBuf>) -> HamburResult<()> {
    collect_all_files_inner(root, output, 0)
}

fn collect_all_files_inner(dir: &Path, output: &mut Vec<PathBuf>, depth: usize) -> HamburResult<()> {
    if depth > 50 {
        return Ok(());
    }
    let read_dir = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) if depth > 0 => {
            // Subdirectory not readable (e.g. PermissionDenied) - skip gracefully
            return Ok(());
        }
        Err(e) => {
            // Top-level root directory not readable
            return Err(HamburError::InvalidCommand(format!(
                "Cannot read directory {}: {e}",
                dir.display()
            )));
        }
    };
    for entry in read_dir {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            let _ = collect_all_files_inner(&path, output, depth + 1);
        } else if ft.is_file() {
            output.push(path);
        }
    }
    Ok(())
}

pub(crate) fn path_for_search_result(
    resolved: &hambur_sandbox::SandboxPathResolution,
    file: &Path,
) -> String {
    if resolved.host_path.is_file() {
        return resolved.sandbox_path.clone();
    }
    let relative = file
        .strip_prefix(&resolved.host_path)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/");
    if relative.is_empty() {
        resolved.sandbox_path.clone()
    } else {
        format!(
            "{}/{}",
            resolved.sandbox_path.trim_end_matches('/'),
            relative.trim_start_matches('/')
        )
    }
}

pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let mut p_idx = 0;
    let mut t_idx = 0;
    let mut star_idx = None;
    let mut match_idx = 0;

    while t_idx < t.len() {
        if p_idx < p.len()
            && (p[p_idx] == '?'
                || p[p_idx].to_ascii_lowercase() == t[t_idx].to_ascii_lowercase())
        {
            p_idx += 1;
            t_idx += 1;
        } else if p_idx < p.len() && p[p_idx] == '*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
        } else if let Some(star) = star_idx {
            p_idx = star + 1;
            match_idx += 1;
            t_idx = match_idx;
        } else {
            return false;
        }
    }

    while p_idx < p.len() && p[p_idx] == '*' {
        p_idx += 1;
    }

    p_idx == p.len()
}

pub(crate) fn matches_file_pattern(pattern: &str, file_name: &str, relative: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern.contains('*') || pattern.contains('?') {
        glob_match(pattern, file_name) || glob_match(pattern, relative)
    } else {
        let pat_lower = pattern.to_ascii_lowercase();
        file_name.to_ascii_lowercase().contains(&pat_lower)
            || relative.to_ascii_lowercase().contains(&pat_lower)
    }
}

pub(crate) fn database_path(bootstrap: &AppBootstrap) -> PathBuf {
    PathBuf::from(&bootstrap.app_files_dir).join("hambur.db")
}

#[allow(dead_code)]
pub(crate) fn is_known_binary_content_type(content_type: &str) -> bool {
    let lower = content_type.to_ascii_lowercase();
    let mime = lower.split(';').next().unwrap_or("").trim();
    if mime == "image/svg+xml" {
        return false;
    }
    mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
        || mime == "application/pdf"
        || mime == "application/zip"
        || mime == "application/gzip"
        || mime == "application/x-tar"
        || mime == "application/octet-stream"
        || mime == "application/vnd.android.package-archive"
        || mime.ends_with("wasm")
        || mime.ends_with("protobuf")
}

#[allow(dead_code)]
pub(crate) fn is_binary_bytes(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let sample_len = bytes.len().min(4096);
    let sample = &bytes[..sample_len];
    if sample.contains(&0) {
        return true;
    }
    let non_text = sample
        .iter()
        .filter(|&&b| (b < 0x20 && b != b'\t' && b != b'\n' && b != b'\r') || b == 0x7f)
        .count();
    if (non_text * 100) / sample_len > 5 {
        return true;
    }
    // If the entire buffer is valid UTF-8, it is definitely text!
    if std::str::from_utf8(bytes).is_ok() {
        return false;
    }
    // If the whole buffer failed, test the sample up to char boundary
    match std::str::from_utf8(sample) {
        Ok(_) => false,
        Err(e) => {
            // error_len().is_some() indicates an invalid byte sequence was encountered (binary).
            // error_len().is_none() means the slice ended cleanly on an incomplete multi-byte character (valid UTF-8 prefix).
            e.error_len().is_some()
        }
    }
}

pub(crate) fn parse_image_dimensions(bytes: &[u8]) -> (u32, u32) {
    if bytes.len() >= 24 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        let width = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        let height = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        return (width, height);
    }
    if bytes.len() >= 10 && (&bytes[0..6] == b"GIF87a" || &bytes[0..6] == b"GIF89a") {
        let width = u16::from_le_bytes([bytes[6], bytes[7]]) as u32;
        let height = u16::from_le_bytes([bytes[8], bytes[9]]) as u32;
        return (width, height);
    }
    if bytes.len() >= 4 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        let mut i = 2;
        while i + 4 < bytes.len() {
            if bytes[i] != 0xFF {
                i += 1;
                continue;
            }
            while i < bytes.len() && bytes[i] == 0xFF {
                i += 1;
            }
            if i >= bytes.len() {
                break;
            }
            let marker = bytes[i];
            i += 1;
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) {
                continue;
            }
            if i + 2 > bytes.len() {
                break;
            }
            let len = u16::from_be_bytes([bytes[i], bytes[i + 1]]) as usize;
            if (0xC0..=0xCF).contains(&marker) && marker != 0xC4 && marker != 0xC8 && marker != 0xCC {
                if i + 7 <= bytes.len() {
                    let height = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]) as u32;
                    let width = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
                    return (width, height);
                }
            }
            if len < 2 {
                break;
            }
            i += len;
        }
    }
    if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        if &bytes[12..16] == b"VP8 " && bytes.len() >= 30 {
            let width = (u16::from_le_bytes([bytes[26], bytes[27]]) & 0x3FFF) as u32;
            let height = (u16::from_le_bytes([bytes[28], bytes[29]]) & 0x3FFF) as u32;
            return (width, height);
        } else if &bytes[12..16] == b"VP8L" && bytes.len() >= 25 {
            let b0 = bytes[21] as u32;
            let b1 = bytes[22] as u32;
            let b2 = bytes[23] as u32;
            let b3 = bytes[24] as u32;
            let width = 1 + (((b1 & 0x3F) << 8) | b0);
            let height = 1 + (((b3 & 0x0F) << 10) | (b2 << 2) | ((b1 & 0xC0) >> 6));
            return (width, height);
        } else if &bytes[12..16] == b"VP8X" && bytes.len() >= 30 {
            let width = 1 + (bytes[24] as u32 | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16));
            let height = 1 + (bytes[27] as u32 | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16));
            return (width, height);
        }
    }
    (0, 0)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_binary_bytes_multilingual_utf8() {
        let text = "Unicode UTF-8 多语种测试：\n\
            中文：你好世界，这是一段纯文本。\n\
            Ελληνικά: Γειά σου κόσμε!\n\
            Русский: Здравствуй, мир!\n\
            العربية: مرحباً بالعالم\n\
            עברית: שלום עולם\n\
            日本語: こんにちは世界\n\
            한국어: 안녕하세요 세계\n";
        let repeated = text.repeat(30);
        assert!(!is_binary_bytes(repeated.as_bytes()));
    }

    #[test]
    fn test_is_binary_bytes_truncated_boundary() {
        let text = "你好世界，测试截断边界。";
        let bytes = text.as_bytes();
        assert!(!is_binary_bytes(&bytes[..2]));
    }

    #[test]
    fn test_is_binary_bytes_binary() {
        assert!(is_binary_bytes(b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR"));
        assert!(is_binary_bytes(&[0u8; 100]));
        let random = (0..500).map(|i| (i % 256) as u8).collect::<Vec<_>>();
        assert!(is_binary_bytes(&random));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.jpg", "crh0207.jpg"));
        assert!(glob_match("*.JPG", "crh0207.jpg"));
        assert!(glob_match("*.jpg", "crh0207.JPG"));
        assert!(!glob_match("*.jpg", "crh0207.png"));
        assert!(glob_match("crh*", "crh0207.jpg"));
        assert!(glob_match("*0207*", "crh0207.jpg"));
        assert!(glob_match("*.min.js", "app.min.js"));
        assert!(!glob_match("*.min.js", "app.js"));
        assert!(glob_match("?a?", "cat"));
        assert!(!glob_match("?a?", "cats"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*.*", "file.ext"));
        assert!(!glob_match("*.*", "no_ext"));
    }

    #[test]
    fn test_matches_file_pattern() {
        // Glob pattern
        assert!(matches_file_pattern("*.jpg", "crh0207.jpg", "hsr_img/crh0207.jpg"));
        assert!(matches_file_pattern("*.jpg", "crh0207.JPG", "hsr_img/crh0207.JPG"));
        assert!(matches_file_pattern("hsr_img/*.jpg", "crh0207.jpg", "hsr_img/crh0207.jpg"));
        assert!(!matches_file_pattern("*.png", "crh0207.jpg", "hsr_img/crh0207.jpg"));

        // Substring pattern (no wildcard)
        assert!(matches_file_pattern("crh0207", "crh0207.jpg", "hsr_img/crh0207.jpg"));
        assert!(matches_file_pattern("CRH0207", "crh0207.jpg", "hsr_img/crh0207.jpg"));
        assert!(matches_file_pattern("hsr", "crh0207.jpg", "hsr_img/crh0207.jpg"));
        assert!(!matches_file_pattern("not_found", "crh0207.jpg", "hsr_img/crh0207.jpg"));
    }

    #[test]
    fn test_collect_all_files() {
        let temp_dir = std::env::temp_dir().join(format!("hambur_test_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let _ = fs::create_dir_all(temp_dir.join("sub1/sub2"));
        let _ = fs::create_dir_all(temp_dir.join("sub3"));
        let _ = fs::write(temp_dir.join("file1.txt"), "hello");
        let _ = fs::write(temp_dir.join("sub1/sub2/file2.txt"), "world");
        let _ = fs::write(temp_dir.join("sub3/file3.txt"), "foo");

        let mut output = Vec::new();
        let res = collect_all_files(&temp_dir, &mut output);
        assert!(res.is_ok());
        assert_eq!(output.len(), 3);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
