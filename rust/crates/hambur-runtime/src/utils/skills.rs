use crate::*;

pub(crate) fn parse_frontmatter(raw: &str) -> HashMap<String, Vec<String>> {
    if !raw.starts_with("---\n") {
        return HashMap::new();
    }
    let Some(end) = raw[4..].find("\n---") else {
        return HashMap::new();
    };
    let yaml = &raw[4..4 + end];
    yaml.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || !trimmed.contains(':') {
                return None;
            }
            let key = trimmed.split_once(':')?.0.trim().to_string();
            let value = trimmed.split_once(':')?.1.trim();
            Some((key, parse_frontmatter_value(value)))
        })
        .collect()
}

pub(crate) fn parse_frontmatter_value(value: &str) -> Vec<String> {
    let unquoted = value.trim().trim_matches('"').trim_matches('\'');
    if unquoted.starts_with('[') && unquoted.ends_with(']') {
        return unquoted
            .trim_start_matches('[')
            .trim_end_matches(']')
            .split(',')
            .map(|item| item.trim().trim_matches('"').trim_matches('\'').to_string())
            .filter(|item| !item.is_empty())
            .collect();
    }
    if unquoted.is_empty() {
        Vec::new()
    } else {
        vec![unquoted.to_string()]
    }
}

pub(crate) fn strip_frontmatter(raw: &str) -> String {
    if !raw.starts_with("---\n") {
        return raw.to_string();
    }
    let Some(end) = raw[4..].find("\n---") else {
        return raw.to_string();
    };
    raw[4 + end + 4..].trim_start_matches('\n').to_string()
}

pub(crate) fn seed_bundled_skills(root: &Path) -> HamburResult<()> {
    fs::create_dir_all(root)
        .map_err(|error| HamburError::Internal(format!("create skills root: {error}")))?;
    for bundled in BUNDLED_SKILLS {
        let destination = safe_join(root, bundled.relative_path)?;
        if destination.exists() {
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| HamburError::Internal(format!("create skill dir: {error}")))?;
        }
        fs::write(&destination, bundled.content)
            .map_err(|error| HamburError::Internal(format!("seed skill: {error}")))?;
    }
    Ok(())
}

pub(crate) fn is_bundled_skill_path(path: &str) -> bool {
    BUNDLED_SKILLS
        .iter()
        .any(|bundled| bundled.relative_path == path)
}

pub(crate) fn linked_skill_files_json(skill_dir: &Path) -> String {
    let mut groups = serde_json::Map::new();
    for child in ["references", "templates", "scripts", "assets"] {
        let dir = skill_dir.join(child);
        if !dir.is_dir() {
            continue;
        }
        let files = list_relative_files(&dir)
            .unwrap_or_default()
            .into_iter()
            .map(|path| Value::String(format!("{child}/{path}")))
            .collect::<Vec<_>>();
        if !files.is_empty() {
            groups.insert(child.to_string(), Value::Array(files));
        }
    }
    Value::Object(groups).to_string()
}

pub(crate) fn skill_summary_json(skill: RuntimeSkillSummary) -> Value {
    json!({
        "name": skill.name,
        "description": skill.description,
        "category": skill.category,
        "path": format!("{SANDBOX_SKILLS_PATH}/{}", skill.path),
        "tags": skill.tags
    })
}

pub(crate) fn skill_detail_json(detail: RuntimeSkillDetail, selected_file: bool) -> Value {
    if selected_file {
        return json!({
            "success": true,
            "name": detail.summary.name,
            "file_path": detail.selected_file_path,
            "path": format!("{SANDBOX_SKILLS_PATH}/{}/{}", detail.skill_dir_path, detail.selected_file_path),
            "content": detail.selected_file_content,
            "summary": "skill file loaded"
        });
    }
    let linked_files =
        serde_json::from_str::<Value>(&detail.linked_files_json).unwrap_or_else(|_| json!({}));
    json!({
        "success": true,
        "name": detail.summary.name,
        "description": detail.summary.description,
        "category": detail.summary.category,
        "path": format!("{SANDBOX_SKILLS_PATH}/{}", detail.summary.path),
        "skill_dir": format!("{SANDBOX_SKILLS_PATH}/{}", detail.skill_dir_path),
        "linked_files": linked_files,
        "content": detail.content,
        "hint": "To view linked files, call skill_view(name, file_path) where file_path is e.g. references/api.md, templates/config.yaml, or scripts/setup.sh."
    })
}

pub(crate) fn disabled_skill_paths_from_snapshot(snapshot: SettingsSnapshot) -> HashSet<String> {
    snapshot
        .settings
        .into_iter()
        .filter_map(|setting| {
            setting
                .key
                .strip_prefix("skill_enabled:")
                .filter(|_| setting.value == "false")
                .map(ToString::to_string)
        })
        .collect()
}

pub(crate) fn disabled_tool_names_from_snapshot(snapshot: &SettingsSnapshot) -> HashSet<String> {
    snapshot
        .settings
        .iter()
        .filter_map(|setting| {
            setting
                .key
                .strip_prefix("tool_enabled:")
                .filter(|_| setting.value == "false")
                .map(ToString::to_string)
        })
        .collect()
}

pub(crate) fn format_skills_index_prompt(skills: Vec<RuntimeSkillSummary>) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut prompt = String::new();
    prompt.push_str("Hambur has a local Skills system at /var/hambur/skills. This directory is shared by all chat sessions and visible inside the Linux sandbox.\n");
    prompt.push_str("Skills are reusable task instructions, references, scripts, and templates. They are loaded through progressive disclosure: use `skills_list` for compact metadata, then `skill_view` to inspect a skill before following its detailed workflow.\n");
    prompt.push_str("Available skills:\n");
    for skill in skills {
        prompt.push_str("- ");
        prompt.push_str(&skill.name);
        if !skill.category.is_empty() {
            prompt.push_str(" [");
            prompt.push_str(&skill.category);
            prompt.push(']');
        }
        prompt.push_str(": ");
        prompt.push_str(
            &skill
                .description
                .chars()
                .take(SKILL_MAX_DESCRIPTION_CHARS)
                .collect::<String>(),
        );
        prompt.push('\n');
    }
    prompt.trim().to_string()
}

pub(crate) const SANDBOX_SKILLS_PATH: &str = "/var/hambur/skills";

pub(crate) const SKILL_MAX_LINKED_FILE_BYTES: u64 = 512_000;

pub(crate) const SKILL_MAX_DESCRIPTION_CHARS: usize = 320;

pub(crate) const BUNDLED_SKILLS: &[BundledSkillFile] = &[BundledSkillFile {
    relative_path: "system/skill-creator/SKILL.md",
    content: include_str!("../../assets/skills/system/skill-creator/SKILL.md"),
}];

pub(crate) struct BundledSkillFile {
    pub(crate) relative_path: &'static str,
    pub(crate) content: &'static str,
}
