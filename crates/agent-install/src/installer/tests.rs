use super::*;
use tempfile::TempDir;

struct Fixture {
    tmp: TempDir,
    home: PathBuf,
    roots: Vec<PathBuf>,
    contents: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let root = tmp.path().join("Applications");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&root).unwrap();
        let contents = root.join("Wealthlog.app/Contents");
        bundle(&contents, "0.0.3", "original");
        Self {
            tmp,
            home,
            roots: vec![root],
            contents,
        }
    }

    fn installer(&self) -> Installer {
        Installer::new(
            self.home.clone(),
            self.contents.clone(),
            self.roots.clone(),
            "0.0.3".into(),
            true,
        )
    }

    fn copy(&self, name: &str, version: &str, text: &str) -> PathBuf {
        let contents = self.roots[0].join(format!("{name}.app/Contents"));
        bundle(&contents, version, text);
        contents
    }

    fn launch(&self, contents: PathBuf, version: &str) -> Installer {
        let mut installer = Installer::new(
            self.home.clone(),
            contents,
            self.roots.clone(),
            version.into(),
            true,
        );
        installer.startup();
        installer
    }
}

fn bundle(contents: &Path, version: &str, text: &str) {
    let skill = contents.join("Resources/skills/wealthlog");
    fs::create_dir_all(contents.join("Helpers")).unwrap();
    let cli = contents.join("Helpers/wealthlog");
    fs::write(&cli, "cli").unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
    for file in SKILL_FILES {
        let path = skill.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, if *file == "VERSION" { version } else { text }).unwrap();
    }
    let mut files = BTreeMap::new();
    collect_files(&skill, &skill, &mut files).unwrap();
    let manifest = Manifest {
        format_version: 1,
        version: version.into(),
        arch: if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        }
        .into(),
        cli_sha256: hash(b"cli"),
        skill_sha256: skill_digest(&files),
        skill_files: files,
    };
    fs::write(
        contents.join("Resources/agent-bundle.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
}

#[test]
fn first_install_repeat_and_modes_preserve_existing_directories() {
    let f = Fixture::new();
    fs::create_dir(f.home.join(".agents")).unwrap();
    fs::set_permissions(f.home.join(".agents"), fs::Permissions::from_mode(0o750)).unwrap();
    let mut i = f.installer();
    i.startup();
    assert_eq!(i.status().cli.state, "provided_here", "{:?}", i.status());
    assert_eq!(i.status().skill.state, "provided_here");
    let before = fs::read(i.record_path()).unwrap();
    i.startup();
    assert_eq!(before, fs::read(i.record_path()).unwrap());
    for (path, mode) in [
        (i.base(), 0o700),
        (i.record_path(), 0o600),
        (f.home.join(".agents"), 0o750),
        (f.home.join(".local/bin"), 0o755),
        (f.home.join(".agents/skills"), 0o755),
    ] {
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            mode
        );
    }
}

#[test]
fn endpoint_deletion_is_persisted_and_other_tool_remains_active() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    fs::remove_file(i.path(Tool::Skill)).unwrap();
    i.startup();
    assert!(!i.load().unwrap().unwrap().skill.desired);
    assert_eq!(i.status().skill.state, "removed");
    assert_eq!(object(&i.path(Tool::Skill)).unwrap(), Object::Missing);
    i.startup();
    assert_eq!(object(&i.path(Tool::Skill)).unwrap(), Object::Missing);
    i.action(Action::Reinstall, Some(Tool::Skill)).unwrap();
    assert_eq!(i.status().skill.state, "provided_here");
}

#[test]
fn dangling_current_and_endpoint_are_repaired_not_removed() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    fs::remove_file(i.current()).unwrap();
    assert_eq!(
        object(&i.path(Tool::Skill)).unwrap(),
        Object::Link(i.target(Tool::Skill))
    );
    i.startup();
    assert_eq!(i.status().skill.state, "provided_here");
    let moved = f.roots[0].join("Moved.app/Contents");
    fs::create_dir_all(moved.parent().unwrap()).unwrap();
    fs::rename(&f.contents, &moved).unwrap();
    let moved_i = f.launch(moved, "0.0.3");
    assert_eq!(
        moved_i.status().cli.state,
        "provided_here",
        "{:?}",
        moved_i.status()
    );
}

#[test]
fn no_record_or_corrupt_record_preserves_all_existing_paths() {
    for corrupt in [false, true] {
        let f = Fixture::new();
        let mut i = f.installer();
        fs::create_dir_all(i.path(Tool::Skill)).unwrap();
        fs::write(i.path(Tool::Skill).join("mine"), "user").unwrap();
        if corrupt {
            fs::create_dir_all(i.base()).unwrap();
            fs::write(i.record_path(), "broken").unwrap();
        }
        i.startup();
        assert_eq!(i.status().skill.state, "user_managed");
        assert_eq!(i.status().cli.state, "needs_repair");
        assert_eq!(
            fs::read_to_string(i.path(Tool::Skill).join("mine")).unwrap(),
            "user"
        );
        assert_eq!(object(&i.path(Tool::Cli)).unwrap(), Object::Missing);
    }
}

#[test]
fn damaged_record_can_adopt_complete_chain_missing_entry_stays_removed() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    fs::write(i.record_path(), "broken").unwrap();
    fs::remove_file(i.path(Tool::Skill)).unwrap();
    i.action(Action::Repair, None).unwrap();
    assert_eq!(i.status().cli.state, "provided_here");
    assert_eq!(i.status().skill.state, "removed");
    i.startup();
    assert_eq!(i.status().skill.state, "removed");
}

#[test]
fn removed_link_reappearing_requires_explicit_choice() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    i.action(Action::Remove, Some(Tool::Cli)).unwrap();
    symlink(i.target(Tool::Cli), i.path(Tool::Cli)).unwrap();
    i.startup();
    assert_eq!(i.status().cli.state, "state_conflict");
    assert!(!i.load().unwrap().unwrap().cli.desired);
    i.action(Action::KeepRemoved, Some(Tool::Cli)).unwrap();
    assert_eq!(i.status().cli.state, "removed");
    symlink(i.target(Tool::Cli), i.path(Tool::Cli)).unwrap();
    i.action(Action::Restore, Some(Tool::Cli)).unwrap();
    assert_eq!(i.status().cli.state, "provided_here");
}

#[test]
fn both_removed_prevents_automatic_provider_updates() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    for tool in [Tool::Cli, Tool::Skill] {
        i.action(Action::Remove, Some(tool)).unwrap();
    }
    let next = f.copy("New", "0.0.4", "next");
    let next_i = f.launch(next, "0.0.4");
    assert_eq!(
        fs::read_link(next_i.current()).unwrap(),
        fs::canonicalize(f.contents).unwrap()
    );
}

#[test]
fn copy_competition_same_version_newer_older_and_explicit_switch() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    let equal = f.copy("Equal", "0.0.3", "original");
    let mut equal_i = f.launch(equal.clone(), "0.0.3");
    assert_eq!(equal_i.status().cli.state, "provided_elsewhere");
    equal_i.action(Action::UseCopy, None).unwrap();
    assert_eq!(equal_i.status().cli.state, "provided_here");
    let different = f.copy("Different", "0.0.3", "different");
    let mut different_i = f.launch(different, "0.0.3");
    assert_eq!(different_i.status().cli.state, "provided_elsewhere");
    different_i.action(Action::UseCopy, None).unwrap();
    assert_eq!(different_i.status().cli.state, "provided_here");
    let newer = f.copy("Newer", "0.1.0", "newer");
    let newer_i = f.launch(newer, "0.1.0");
    assert_eq!(newer_i.status().cli.state, "provided_here");
    let mut older_i = f.launch(equal, "0.0.3");
    assert_eq!(older_i.status().cli.state, "provided_elsewhere");
    assert!(older_i.action(Action::UseCopy, None).is_err());
}

#[test]
fn same_path_replacement_can_downgrade_without_cross_path_takeover() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    bundle(&f.contents, "0.0.1", "older");
    let replaced = f.launch(f.contents.clone(), "0.0.1");
    assert_eq!(replaced.status().version.as_deref(), Some("0.0.1"));
    assert_eq!(replaced.status().cli.state, "provided_here");
}

#[test]
fn moved_different_digest_same_version_does_not_take_over_automatically() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    fs::remove_dir_all(f.contents.parent().unwrap()).unwrap();
    let other = f.copy("Other", "0.0.3", "changed");
    let mut moved = f.launch(other, "0.0.3");
    assert_ne!(moved.status().cli.state, "provided_here");
    moved.action(Action::UseCopy, None).unwrap();
    assert_eq!(moved.status().cli.state, "provided_here");
}

#[test]
fn external_current_or_endpoint_is_never_overwritten() {
    for endpoint in [false, true] {
        let f = Fixture::new();
        let mut i = f.installer();
        i.startup();
        let path = if endpoint {
            i.path(Tool::Cli)
        } else {
            i.current()
        };
        fs::remove_file(&path).unwrap();
        symlink("/external", &path).unwrap();
        i.startup();
        assert_eq!(i.status().cli.state, "conflict");
        assert!(i.action(Action::Reinstall, Some(Tool::Cli)).is_err());
        assert_eq!(fs::read_link(&path).unwrap(), PathBuf::from("/external"));
        if endpoint {
            i.action(Action::Remove, Some(Tool::Cli)).unwrap();
            assert_eq!(fs::read_link(&path).unwrap(), PathBuf::from("/external"));
            assert!(!i.load().unwrap().unwrap().cli.desired);
        }
    }
}

#[test]
fn interrupted_publication_old_new_and_partial_are_repairable() {
    for stage in [1, 2, 3] {
        let f = Fixture::new();
        let mut i = f.installer();
        i.interrupt = Some(stage);
        i.startup();
        i.interrupt = None;
        let mut restarted = f.installer();
        restarted.startup();
        if stage != 3 {
            assert_eq!(
                restarted.status().cli.state,
                "needs_repair",
                "stage {stage}"
            );
            restarted.action(Action::Repair, None).unwrap();
        }
        assert_eq!(restarted.status().cli.state, "provided_here");
        assert_eq!(restarted.status().skill.state, "provided_here");
    }
    let f = Fixture::new();
    let mut i = f.installer();
    i.interrupt = Some(1);
    i.startup();
    let changes = i.load().unwrap().unwrap().pending.unwrap();
    i.link(&changes[0]).unwrap();
    i.interrupt = None;
    i.startup();
    assert_eq!(i.status().cli.state, "needs_repair");
    i.action(Action::Repair, None).unwrap();
    assert_eq!(i.status().skill.state, "provided_here");
}

#[test]
fn interrupted_external_replacement_conflicts_and_unknown_path_fails() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.interrupt = Some(1);
    i.startup();
    symlink("/external", i.current()).unwrap();
    i.interrupt = None;
    i.startup();
    assert_eq!(i.status().cli.state, "conflict");
    assert!(i.action(Action::Repair, None).is_err());
    assert_eq!(
        fs::read_link(i.current()).unwrap(),
        PathBuf::from("/external")
    );
}

#[test]
fn one_parent_is_file_or_unwritable_other_tool_installs() {
    for tool in [Tool::Cli, Tool::Skill] {
        for file_parent in [true, false] {
            let f = Fixture::new();
            let mut i = f.installer();
            let path = i.path(tool);
            let parent = path.parent().unwrap();
            fs::create_dir_all(parent.parent().unwrap()).unwrap();
            if file_parent {
                fs::write(parent, "blocked").unwrap();
            } else {
                fs::create_dir(parent).unwrap();
                fs::set_permissions(parent, fs::Permissions::from_mode(0o555)).unwrap();
            }
            i.startup();
            let status = i.status();
            let (failed, other) = match tool {
                Tool::Cli => (&status.cli, &status.skill),
                Tool::Skill => (&status.skill, &status.cli),
            };
            assert_eq!(failed.state, "failed", "{status:?}");
            assert_eq!(other.state, "provided_here", "{status:?}");
            if !file_parent {
                fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }
}

#[test]
fn publication_permission_failure_does_not_leave_pending_or_block_the_other_tool() {
    for tool in [Tool::Cli, Tool::Skill] {
        let f = Fixture::new();
        let mut i = f.installer();
        i.link_failure = Some(i.path(tool));
        i.startup();
        let record = i.load().unwrap().unwrap();
        assert!(record.pending.is_none());
        assert_eq!(record.entry(tool).result, "failed");
        assert_eq!(record.entry(tool).phase, "idle");
        assert!(record.entry(tool).desired);
        let other = if tool == Tool::Cli {
            Tool::Skill
        } else {
            Tool::Cli
        };
        assert_eq!(record.entry(other).result, "installed");
        assert_eq!(
            object(&i.path(other)).unwrap(),
            Object::Link(i.target(other))
        );
        assert_eq!(object(&i.path(tool)).unwrap(), Object::Missing);
        let (failed, installed) = if tool == Tool::Cli {
            (i.status().cli, i.status().skill)
        } else {
            (i.status().skill, i.status().cli)
        };
        assert_eq!(failed.state, "failed");
        assert_eq!(installed.state, "provided_here");
        i.startup();
        assert!(i.load().unwrap().unwrap().pending.is_none());
        assert!(!i.last_error.as_ref().unwrap().contains("Interrupted"));
        i.link_failure = None;
        i.startup();
        assert_eq!(i.status().cli.state, "provided_here");
        assert_eq!(i.status().skill.state, "provided_here");
    }
}

#[test]
fn failure_to_publish_current_preserves_the_shared_journal_and_stops_endpoints() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.link_failure = Some(i.current());
    i.startup();
    assert!(i.load().unwrap().unwrap().pending.is_some());
    assert_eq!(object(&i.path(Tool::Cli)).unwrap(), Object::Missing);
    assert_eq!(object(&i.path(Tool::Skill)).unwrap(), Object::Missing);
    i.link_failure = None;
    i.action(Action::Repair, None).unwrap();
    assert_eq!(i.status().cli.state, "provided_here");
    assert_eq!(i.status().skill.state, "provided_here");
}

#[test]
fn failed_publication_with_a_changed_target_keeps_recovery_data_and_continues_other_links() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.link_failure = Some(i.path(Tool::Cli));
    i.link_failure_after_rename = true;
    i.startup();
    let record = i.load().unwrap().unwrap();
    let pending = record.pending.as_ref().unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].path, i.path(Tool::Cli));
    assert_eq!(record.cli.result, "failed");
    assert_eq!(record.skill.result, "installed");
    assert!(record.skill.installed);
    assert_eq!(
        fs::read_link(i.path(Tool::Skill)).unwrap(),
        i.target(Tool::Skill)
    );
    i.link_failure = None;
    i.action(Action::Repair, None).unwrap();
    assert!(i.load().unwrap().unwrap().pending.is_none());
    assert_eq!(i.status().cli.state, "provided_here");
}

#[test]
fn repair_and_takeover_actions_are_only_offered_when_relevant() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    assert!(!i.status().can_repair);
    assert!(!i.status().can_use_copy);
    let equal = f.copy("Other", "0.0.3", "original");
    let other = f.launch(equal, "0.0.3");
    assert!(!other.status().can_repair);
    assert!(other.status().can_use_copy);
    fs::remove_file(i.current()).unwrap();
    assert!(i.status().can_repair);
    assert!(!i.status().can_use_copy);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_acl_denies_creation_despite_0755_mode_and_the_other_tool_still_installs() {
    for tool in [Tool::Cli, Tool::Skill] {
        let f = Fixture::new();
        let mut i = f.installer();
        let parent = i.path(tool).parent().unwrap().to_path_buf();
        fs::create_dir_all(&parent).unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(std::process::Command::new("/bin/chmod")
            .args(["+a", "everyone deny add_file"])
            .arg(&parent)
            .status()
            .unwrap()
            .success());
        assert_eq!(
            fs::metadata(&parent).unwrap().permissions().mode() & 0o777,
            0o755
        );
        i.startup();
        let record = i.load().unwrap().unwrap();
        // Restore only this test-created directory before assertions/cleanup.
        assert!(std::process::Command::new("/bin/chmod")
            .arg("-N")
            .arg(&parent)
            .status()
            .unwrap()
            .success());
        assert!(record.pending.is_none());
        assert_eq!(record.entry(tool).result, "failed");
        let other = if tool == Tool::Cli {
            Tool::Skill
        } else {
            Tool::Cli
        };
        assert_eq!(record.entry(other).result, "installed");
        i.startup();
        assert_eq!(i.status().cli.state, "provided_here");
        assert_eq!(i.status().skill.state, "provided_here");
    }
}

#[test]
fn reappeared_disabled_endpoint_keeps_its_conflict_while_the_other_tool_upgrades() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    i.action(Action::Remove, Some(Tool::Cli)).unwrap();
    symlink(i.target(Tool::Cli), i.path(Tool::Cli)).unwrap();
    let before = fs::symlink_metadata(i.path(Tool::Cli)).unwrap();
    let next = f.copy("Next", "0.0.4", "next");
    let upgraded = f.launch(next, "0.0.4");
    assert_eq!(upgraded.status().cli.state, "state_conflict");
    assert_eq!(upgraded.status().skill.state, "provided_here");
    assert_eq!(upgraded.status().version.as_deref(), Some("0.0.4"));
    assert!(!upgraded.load().unwrap().unwrap().cli.desired);
    assert_eq!(
        before.modified().unwrap(),
        fs::symlink_metadata(upgraded.path(Tool::Cli))
            .unwrap()
            .modified()
            .unwrap()
    );
    assert_eq!(
        fs::read_link(upgraded.path(Tool::Cli)).unwrap(),
        upgraded.target(Tool::Cli)
    );
}

#[test]
fn record_write_failure_does_not_publish_links() {
    let f = Fixture::new();
    let mut i = f.installer();
    fs::create_dir_all(i.record_path()).unwrap();
    i.startup();
    assert_eq!(object(&i.current()).unwrap(), Object::Missing);
    assert_eq!(object(&i.path(Tool::Cli)).unwrap(), Object::Missing);
}

#[test]
fn exclusive_lock_skips_startup_without_waiting() {
    let f = Fixture::new();
    let mut i = f.installer();
    let _held = i.lock().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "installer::tests::lock_probe_in_child",
            "--ignored",
        ])
        .env("WEALTHLOG_TEST_LOCK_PATH", i.base().join("install.lock"))
        .output()
        .unwrap();
    assert!(
        child.status.success(),
        "{}",
        String::from_utf8_lossy(&child.stdout)
    );
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
    i.startup();
    assert!(i.last_error.as_ref().unwrap().contains("lock busy"));
    assert_eq!(object(&i.current()).unwrap(), Object::Missing);
}

#[test]
#[ignore = "invoked by exclusive_lock_skips_startup_without_waiting in a child process"]
fn lock_probe_in_child() {
    let path = std::env::var_os("WEALTHLOG_TEST_LOCK_PATH").unwrap();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    assert!(file.try_lock().is_err());
}

#[test]
fn unsupported_locations_and_symlink_escape_do_not_install() {
    let f = Fixture::new();
    let outside = f.tmp.path().join("Downloads/Wealthlog.app/Contents");
    bundle(&outside, "0.0.3", "original");
    let escaped = f.roots[0].join("Escaped.app");
    symlink(outside.parent().unwrap(), &escaped).unwrap();
    for contents in [outside, escaped.join("Contents")] {
        let i = f.launch(contents, "0.0.3");
        assert_eq!(i.status().cli.state, "unsupported_location");
        assert_eq!(object(&i.current()).unwrap(), Object::Missing);
    }
    let mut i = f.installer();
    i.enabled = false;
    i.startup();
    assert!(!i.status().supported);
    assert!(i.action(Action::Reinstall, Some(Tool::Cli)).is_err());
    assert_eq!(object(&i.base()).unwrap(), Object::Missing);
}

#[test]
fn resource_integrity_and_polling_cache_target_invalidation() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    let script = f
        .contents
        .join("Resources/skills/wealthlog/scripts/wealthlog_date.py");
    fs::remove_file(&script).unwrap();
    // Polling uses the cached conclusion and never rescans skill files.
    assert_eq!(i.status().skill.state, "provided_here");
    assert!(i.action(Action::Reinstall, Some(Tool::Cli)).is_err());
    assert!(!fs::exists(&script).unwrap());
    bundle(&f.contents, "0.0.3", "original");
    let newer = f.copy("Newer", "0.0.4", "next");
    let _other = f.launch(newer, "0.0.4");
    assert_eq!(i.status().skill.state, "pending_validation");
    i.startup();
    assert_eq!(i.status().skill.state, "provided_elsewhere");
}

#[test]
fn first_install_rejects_bad_cli_version_digest_or_extra_skill_file() {
    for damage in ["version", "cli", "extra", "mode"] {
        let f = Fixture::new();
        let mut i = f.installer();
        match damage {
            "version" => i.app_version = "9.0.0".into(),
            "cli" => fs::write(f.contents.join("Helpers/wealthlog"), "changed").unwrap(),
            "extra" => {
                fs::write(f.contents.join("Resources/skills/wealthlog/extra"), "extra").unwrap()
            }
            _ => fs::set_permissions(
                f.contents.join("Helpers/wealthlog"),
                fs::Permissions::from_mode(0o644),
            )
            .unwrap(),
        }
        i.startup();
        assert!(i.last_error.is_some());
        assert_eq!(object(&i.current()).unwrap(), Object::Missing);
    }
}

#[test]
fn explicit_repair_or_reinstall_does_not_resurrect_another_deleted_tool() {
    for action in [Action::Repair, Action::Reinstall] {
        let f = Fixture::new();
        let mut i = f.installer();
        i.startup();
        fs::remove_file(i.path(Tool::Skill)).unwrap();
        i.action(action, Some(Tool::Cli)).unwrap();
        assert_eq!(i.status().skill.state, "removed");
        assert_eq!(object(&i.path(Tool::Skill)).unwrap(), Object::Missing);
    }
}

#[test]
fn interrupted_uninspectable_endpoint_is_failure_not_ownership_conflict() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.interrupt = Some(1);
    i.startup();
    let parent = i.path(Tool::Cli).parent().unwrap().to_path_buf();
    fs::remove_dir(&parent).unwrap();
    fs::write(&parent, "not a directory").unwrap();
    i.interrupt = None;
    i.startup();
    assert_eq!(i.status().cli.state, "failed");
    assert!(!i
        .last_error
        .as_ref()
        .unwrap()
        .contains("Ownership conflict"));
    assert_eq!(fs::read_to_string(parent).unwrap(), "not a directory");
    assert!(i.load().unwrap().unwrap().pending.is_some());
}

#[test]
fn concurrent_startups_leave_one_complete_owned_installation() {
    let f = Fixture::new();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let handles: Vec<_> = (0..6)
        .map(|_| {
            let barrier = barrier.clone();
            let mut i = f.installer();
            std::thread::spawn(move || {
                barrier.wait();
                i.startup();
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let mut i = f.installer();
    i.startup();
    assert_eq!(i.status().cli.state, "provided_here");
    assert_eq!(i.status().skill.state, "provided_here");
    assert!(i.load().unwrap().unwrap().pending.is_none());
}

#[test]
fn failed_remove_keeps_opt_out_and_persists_the_error() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    let parent = i.path(Tool::Cli).parent().unwrap().to_path_buf();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    let result = i.action(Action::Remove, Some(Tool::Cli));
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    let record = i.load().unwrap().unwrap();
    assert!(!record.cli.desired);
    assert!(record.cli.error.is_some());
    assert_eq!(
        object(&i.path(Tool::Cli)).unwrap(),
        Object::Link(i.target(Tool::Cli))
    );
    i.action(Action::KeepRemoved, Some(Tool::Cli)).unwrap();
    assert_eq!(i.status().cli.state, "removed");
}

#[test]
fn observing_a_different_target_invalidates_cache_even_if_old_target_returns() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    let saved = fs::read(i.record_path()).unwrap();
    let old_target = fs::read_link(i.current()).unwrap();
    let next = f.copy("Next", "0.0.4", "next");
    let _next_i = f.launch(next, "0.0.4");
    assert_eq!(i.status().skill.state, "pending_validation");
    fs::remove_file(i.current()).unwrap();
    symlink(old_target, i.current()).unwrap();
    fs::write(i.record_path(), saved).unwrap();
    assert_eq!(i.status().skill.state, "pending_validation");
    i.startup();
    assert_eq!(i.status().skill.state, "provided_here");
}

#[test]
fn reinstall_selected_tool_works_while_other_parent_cannot_be_inspected() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    i.action(Action::Remove, Some(Tool::Skill)).unwrap();
    fs::remove_file(i.path(Tool::Cli)).unwrap();
    let parent = i.path(Tool::Cli).parent().unwrap().to_path_buf();
    fs::remove_dir(&parent).unwrap();
    fs::write(&parent, "blocked").unwrap();
    i.action(Action::Reinstall, Some(Tool::Skill)).unwrap();
    assert_eq!(i.status().skill.state, "provided_here");
    assert_eq!(i.status().cli.state, "failed");
}

#[test]
fn semantic_precedence_does_not_treat_build_metadata_as_an_upgrade() {
    let f = Fixture::new();
    bundle(&f.contents, "0.0.3+one", "one");
    let _first = f.launch(f.contents.clone(), "0.0.3+one");
    let second = f.copy("Second", "0.0.3+two", "two");
    let mut second_i = f.launch(second, "0.0.3+two");
    assert_eq!(second_i.status().cli.state, "provided_elsewhere");
    second_i.action(Action::UseCopy, None).unwrap();
    assert_eq!(second_i.status().cli.state, "provided_here");
}

#[test]
fn a_copy_competes_with_the_actual_replacement_version_at_the_owning_path() {
    let f = Fixture::new();
    let mut original = f.installer();
    original.startup();
    bundle(&f.contents, "0.0.5", "new owner contents without launch");
    let other = f.copy("Other", "0.0.4", "older than owner");
    let mut other_i = f.launch(other, "0.0.4");
    assert_eq!(other_i.status().cli.state, "provided_elsewhere");
    assert_eq!(other_i.status().version.as_deref(), Some("0.0.5"));
    assert!(other_i.action(Action::UseCopy, None).is_err());
}

#[test]
fn explicit_repair_preserves_a_valid_chain_and_the_previewed_provider() {
    let f = Fixture::new();
    let mut original = f.installer();
    original.startup();
    let next = f.copy("Next", "0.0.4", "next");
    let mut next_i = Installer::new(f.home.clone(), next, f.roots.clone(), "0.0.4".into(), true);
    let preview = next_i.status().repair_app.unwrap();
    next_i.action(Action::Repair, None).unwrap();
    assert_eq!(next_i.status().provider_app, Some(preview));
    assert_eq!(next_i.status().version.as_deref(), Some("0.0.3"));
    next_i.action(Action::UseCopy, None).unwrap();
    assert_eq!(next_i.status().version.as_deref(), Some("0.0.4"));
}

#[test]
fn an_unwritable_parent_does_not_fail_a_correct_link_and_restoring_it_keeps_that_link() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    let parent = i.path(Tool::Cli).parent().unwrap().to_path_buf();
    let original = fs::read_link(i.path(Tool::Cli)).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    i.startup();
    assert_eq!(i.status().cli.state, "provided_here");
    assert!(i.load().unwrap().unwrap().cli.error.is_none());
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    i.startup();
    assert_eq!(i.status().cli.state, "provided_here");
    assert_eq!(fs::read_link(i.path(Tool::Cli)).unwrap(), original);
}

#[test]
fn an_existing_correct_link_stays_healthy_when_its_parent_becomes_unwritable() {
    let f = Fixture::new();
    let mut i = f.installer();
    i.startup();
    let parent = i.path(Tool::Skill).parent().unwrap().to_path_buf();
    let before = fs::symlink_metadata(i.path(Tool::Skill)).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o555)).unwrap();
    i.startup();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).unwrap();
    let after = fs::symlink_metadata(i.path(Tool::Skill)).unwrap();
    assert_eq!(i.status().skill.state, "provided_here");
    assert!(i.load().unwrap().unwrap().skill.error.is_none());
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(
        fs::read_link(i.path(Tool::Skill)).unwrap(),
        i.target(Tool::Skill)
    );
}

#[test]
#[ignore = "requires WEALTHLOG_TEST_BUNDLE pointing at a freshly built app"]
fn actual_app_bundle_installation_and_scripts_leave_package_unchanged() {
    let app = PathBuf::from(std::env::var_os("WEALTHLOG_TEST_BUNDLE").expect("test app path"));
    let app = fs::canonicalize(app).unwrap();
    let contents = app.join("Contents");
    let manifest = read_manifest(&contents).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let mut i = Installer::new(
        temp.path().to_path_buf(),
        contents.clone(),
        vec![app.parent().unwrap().to_path_buf()],
        manifest.version.clone(),
        true,
    );
    let mut before = BTreeMap::new();
    collect_files(
        &contents.join("Resources/skills/wealthlog"),
        &contents.join("Resources/skills/wealthlog"),
        &mut before,
    )
    .unwrap();
    i.startup();
    assert_eq!(i.status().cli.state, "provided_here", "{:?}", i.status());
    assert_eq!(i.status().skill.state, "provided_here");
    for script in ["wealthlog_date.py", "wealthlog_snapshot.py"] {
        let result = std::process::Command::new("python3")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .arg(i.path(Tool::Skill).join("scripts").join(script))
            .arg("--help")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let mut after = BTreeMap::new();
    collect_files(
        &contents.join("Resources/skills/wealthlog"),
        &contents.join("Resources/skills/wealthlog"),
        &mut after,
    )
    .unwrap();
    assert_eq!(before, after);
    i.action(Action::Remove, Some(Tool::Cli)).unwrap();
    i.startup();
    assert_eq!(i.status().cli.state, "removed");
    i.action(Action::Reinstall, Some(Tool::Cli)).unwrap();
    assert_eq!(i.status().cli.state, "provided_here");
}
