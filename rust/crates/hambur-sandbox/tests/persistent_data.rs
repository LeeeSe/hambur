//! 持久数据（记忆 / 技能 / shared）必须活在 RootFS 之外。
//!
//! 背景：2026-10-05 一次 RootFS 重置把 `sandbox/global/` 整个删掉，用户 7 条记忆和全部
//! 自定义技能一起消失。这些测试钉住三件事：
//! 1. 老布局 `sandbox/global/<name>` 会在构造时迁移到 `persistent/<name>`；
//! 2. 迁移不覆盖目标里已有的（更新的）条目；
//! 3. 沙箱路径 `/var/hambur/{memory,skills}` 仍然解析到持久目录——terminal / 文件工具
//!    看到的挂载点没变，所以它们照旧能读写这些文件。

use std::fs;
use std::path::PathBuf;

use hambur_sandbox::{SandboxAccess, SandboxService};

fn scratch(tag: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("hambur-sandbox-test-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

#[test]
fn legacy_global_dir_is_migrated_into_persistent() {
    let files = scratch("migrate");
    let legacy_memory = files.join("sandbox/global/memory");
    fs::create_dir_all(&legacy_memory).unwrap();
    fs::write(legacy_memory.join("MEMORY.md"), "第一条\n§\n第二条").unwrap();
    let legacy_skill = files.join("sandbox/global/skills/commute-eta");
    fs::create_dir_all(&legacy_skill).unwrap();
    fs::write(legacy_skill.join("SKILL.md"), "# commute-eta").unwrap();

    let service = SandboxService::new(&files).unwrap();

    assert_eq!(
        fs::read_to_string(service.persistent_dir("memory").join("MEMORY.md")).unwrap(),
        "第一条\n§\n第二条"
    );
    assert_eq!(
        fs::read_to_string(
            service
                .persistent_dir("skills")
                .join("commute-eta/SKILL.md")
        )
        .unwrap(),
        "# commute-eta"
    );
    // 旧位置必须清空：reset_rootfs 仍会删 sandbox/global，留在那里等于没迁。
    assert!(!legacy_memory.join("MEMORY.md").exists());
    assert!(!legacy_skill.join("SKILL.md").exists());

    let _ = fs::remove_dir_all(&files);
}

#[test]
fn migration_keeps_data_already_in_persistent() {
    let files = scratch("keep");
    fs::create_dir_all(files.join("persistent/memory")).unwrap();
    fs::write(files.join("persistent/memory/MEMORY.md"), "新的").unwrap();
    fs::create_dir_all(files.join("sandbox/global/memory")).unwrap();
    fs::write(files.join("sandbox/global/memory/MEMORY.md"), "旧的").unwrap();
    fs::create_dir_all(files.join("sandbox/global/memory")).unwrap();
    fs::write(files.join("sandbox/global/memory/ONLY_OLD.md"), "只有旧的").unwrap();

    let service = SandboxService::new(&files).unwrap();

    assert_eq!(
        fs::read_to_string(service.persistent_dir("memory").join("MEMORY.md")).unwrap(),
        "新的"
    );
    // 目标里没有的条目仍然要搬过来，不能因为同名冲突就整目录放弃。
    assert_eq!(
        fs::read_to_string(service.persistent_dir("memory").join("ONLY_OLD.md")).unwrap(),
        "只有旧的"
    );

    let _ = fs::remove_dir_all(&files);
}

#[test]
fn sandbox_paths_still_point_at_persistent_dirs() {
    let files = scratch("resolve");
    let service = SandboxService::new(&files).unwrap();
    let persistent = service.persistent_root();

    let memory = service
        .resolve("", "/var/hambur/memory/MEMORY.md", SandboxAccess::Read)
        .unwrap();
    assert_eq!(memory.host_path, persistent.join("memory/MEMORY.md"));
    assert_eq!(memory.relative_path, "persistent/memory/MEMORY.md");

    let skill = service
        .resolve("", "/var/hambur/skills/demo/SKILL.md", SandboxAccess::Write)
        .unwrap();
    assert_eq!(skill.host_path, persistent.join("skills/demo/SKILL.md"));

    // 沙箱内可见路径保持不变（挂载点没动，terminal 工具照旧）。
    assert_eq!(skill.sandbox_path, "/var/hambur/skills/demo/SKILL.md");
    assert!(skill.writable);

    // 记忆仍然是只读挂载。
    assert_eq!(
        service
            .resolve("", "/var/hambur/memory/MEMORY.md", SandboxAccess::Write)
            .is_err(),
        true
    );

    let _ = fs::remove_dir_all(&files);
}
