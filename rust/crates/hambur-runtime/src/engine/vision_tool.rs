use crate::*;

impl RuntimeEngine {
    pub(crate) fn resolve_view_image_result(
        &self,
        session_id: &str,
        route: &ModelRouteSnapshot,
        route_candidates: &[ModelRouteSnapshot],
        invocation: &ToolInvocation,
        arguments: &Value,
    ) -> ToolResult {
        let path = arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let detail = arguments
            .get("detail")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let detail = if detail.is_empty() {
            match self.database.settings_snapshot() {
                Ok(snapshot) => {
                    match setting_value(&snapshot, "viewImageScaleMode", "resize_fit").as_str() {
                        "original" => "original",
                        _ => "high",
                    }
                }
                Err(_) => "high",
            }
        } else {
            detail
        };
        if path.is_empty() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image path must not be empty",
            );
        }
        if !route.supports_image_input && vision_handoff_target(route_candidates, route).is_none() {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image unavailable: no vision-capable handoff target",
            );
        }

        let normalized_path = normalize_tool_sandbox_path(path);

        // Attempt 1: Resolve from sandbox filesystem
        let found_in_sandbox = match self
            .sandbox
            .resolve(session_id, &normalized_path, SandboxAccess::Read)
        {
            Ok(res) if res.host_path.is_file() => Some(res),
            _ => None,
        };

        let (host_path, sandbox_path, relative_path, mime_type, file_id) = if let Some(res) =
            found_in_sandbox
        {
            let ext = res
                .host_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or_default()
                .to_lowercase();
            let detected_mime = match ext.as_str() {
                "jpg" | "jpeg" => "image/jpeg",
                "png" => "image/png",
                "webp" => "image/webp",
                "gif" => "image/gif",
                "bmp" => "image/bmp",
                "svg" => "image/svg+xml",
                "ico" => "image/x-icon",
                "heic" => "image/heic",
                "heif" => "image/heif",
                _ => "application/octet-stream",
            };
            let existing_db = self
                .database
                .resolve_file_by_sandbox_path(session_id, &res.sandbox_path)
                .ok();
            let (mime, fid) = if let Some(db_f) = existing_db {
                (db_f.mime_type, db_f.id)
            } else {
                let fid = new_id("file");
                let byte_size = fs::metadata(&res.host_path).map(|m| m.len()).unwrap_or(0);
                let scope = if res.root == "workspace"
                    || res.root == "attachments"
                    || res.root == "browser"
                    || res.root == "mounts"
                    || res.root == "offloads"
                {
                    "session".to_string()
                } else {
                    "global".to_string()
                };
                let _ = self
                    .database
                    .upsert_file_record(NewFileRecord {
                        id: fid.clone(),
                        scope,
                        session_id: session_id.to_string(),
                        relative_path: res.relative_path.clone(),
                        sandbox_path: res.sandbox_path.clone(),
                        mime_type: detected_mime.to_string(),
                        byte_size,
                        sha256: String::new(),
                        retention_policy: "delete_with_session".to_string(),
                    });
                (detected_mime.to_string(), fid)
            };
            (res.host_path, res.sandbox_path, res.relative_path, mime, fid)
        } else {
            // Attempt 2: Fall back to database + filestore
            let db_res = self
                .database
                .resolve_file_by_sandbox_path(session_id, &normalized_path);
            let db_file = match db_res {
                Ok(file) => Ok(file),
                Err(_) => self.database.resolve_file_by_sandbox_path(session_id, path),
            };
            let db_file = match db_file {
                Ok(file) => file,
                Err(_) => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("file not found for sandbox path: {path}"),
                    );
                }
            };
            let host_path = match self.filestore.host_path_for_relative(&db_file.relative_path) {
                Ok(hp) if hp.is_file() => hp,
                _ => {
                    return ToolResult::failed(
                        &invocation.tool_call_id,
                        &invocation.name,
                        format!("file not found for sandbox path: {path}"),
                    );
                }
            };
            (
                host_path,
                db_file.sandbox_path,
                db_file.relative_path,
                db_file.mime_type,
                db_file.id,
            )
        };

        if !mime_type.starts_with("image/") {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                "view_image requires an image file",
            );
        }
        let (width, height) = match std::fs::read(&host_path) {
            Ok(bytes) => parse_image_dimensions(&bytes),
            Err(error) => {
                eprintln!("failed to read host image file {}: {error}", host_path.display());
                (0, 0)
            }
        };
        let ext = host_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_lowercase();
        if (width == 0 || height == 0) && ext != "svg" {
            return ToolResult::failed(
                &invocation.tool_call_id,
                &invocation.name,
                format!("view_image requires a valid image file, but '{path}' could not be decoded as an image"),
            );
        }
        let normalized_detail = normalize_image_detail(detail);
        let image_attached_to_next_request =
            route.supports_image_input || vision_handoff_target(route_candidates, route).is_some();
        let content = json!({
            "path": path,
            "resolvedPath": sandbox_path,
            "relativePath": relative_path,
            "hostPath": host_path.to_string_lossy().to_string(),
            "detail": normalized_detail,
            "width": width,
            "height": height,
            "mimeType": mime_type,
            "fileId": file_id,
            "imageAttachedToNextRequest": image_attached_to_next_request
        });
        let context_stub = format!(
            "Image returned by view_image for tool_call_id={}: ImagePart(fileId={}, path={}, mimeType={}, detail={}, width={}, height={})",
            invocation.tool_call_id,
            file_id,
            sandbox_path,
            mime_type,
            normalized_detail,
            width,
            height
        );
        ToolResult {
            tool_call_id: invocation.tool_call_id.clone(),
            tool_name: invocation.name.clone(),
            is_error: false,
            content_json: content.to_string(),
            summary: "Image prepared for vision continuation".to_string(),
            artifacts_json: "[]".to_string(),
            trust_level: "trusted".to_string(),
            truncated: false,
            offloaded_file_id: String::new(),
            offloaded_path: String::new(),
            context_stub,
        }
    }
}
