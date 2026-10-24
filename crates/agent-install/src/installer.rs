use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::{symlink, DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;

const SKILL_FILES: &[&str] = &[
    "SKILL.md",
    "VERSION",
    "references/connection.md",
    "references/operations.md",
    "scripts/wealthlog_date.py",
    "scripts/wealthlog_snapshot.py",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub format_version: u32,
    pub version: String,
    pub arch: String,
    pub cli_sha256: String,
    pub skill_sha256: String,
    pub skill_files: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provider {
    pub contents: PathBuf,
    pub manifest: Manifest,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    Cli,
    Skill,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    desired: bool,
    phase: String,
    result: String,
    error: Option<String>,
    installed: bool,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            desired: true,
            phase: "idle".into(),
            result: "absent".into(),
            error: None,
            installed: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Change {
    path: PathBuf,
    old: Option<PathBuf>,
    new: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    format_version: u32,
    provider: Provider,
    cli: Entry,
    skill: Entry,
    pending: Option<Vec<Change>>,
}

impl Record {
    fn entry(&self, tool: Tool) -> &Entry {
        match tool {
            Tool::Cli => &self.cli,
            Tool::Skill => &self.skill,
        }
    }

    fn entry_mut(&mut self, tool: Tool) -> &mut Entry {
        match tool {
            Tool::Cli => &mut self.cli,
            Tool::Skill => &mut self.skill,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub state: String,
    pub path: PathBuf,
    pub desired: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub supported: bool,
    pub location_supported: bool,
    pub cli: ToolStatus,
    pub skill: ToolStatus,
    pub provider_app: Option<PathBuf>,
    pub current_app: Option<PathBuf>,
    pub repair_app: Option<PathBuf>,
    pub version: Option<String>,
    pub config: serde_json::Value,
    pub error: Option<String>,
    pub can_repair: bool,
    pub can_use_copy: bool,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Reinstall,
    Remove,
    Repair,
    UseCopy,
    Restore,
    KeepRemoved,
}

#[derive(Debug, PartialEq, Eq)]
enum Object {
    Missing,
    Link(PathBuf),
    Other,
}

fn object(path: &Path) -> Result<Object> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => fs::read_link(path)
            .map(Object::Link)
            .map_err(|e| format!("Cannot read link {}: {e}", path.display())),
        Ok(_) => Ok(Object::Other),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Object::Missing),
        Err(e) => Err(format!("Cannot inspect {}: {e}", path.display())),
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn skill_digest(files: &BTreeMap<String, String>) -> String {
    let mut data = String::new();
    for (name, digest) in files {
        data.push_str(name);
        data.push('\0');
        data.push_str(digest);
        data.push('\n');
    }
    hash(data.as_bytes())
}

fn plain_file(path: &Path) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("Expected a regular file: {}", path.display()));
    }
    fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn collect_files(root: &Path, dir: &Path, files: &mut BTreeMap<String, String>) -> Result<()> {
    for item in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let path = item.path();
        let kind = item.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            collect_files(root, &path, files)?;
        } else if kind.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_str()
                .ok_or("Non UTF-8 skill filename")?
                .to_string();
            files.insert(name, hash(&plain_file(&path)?));
        } else {
            return Err(format!("Unsupported skill object: {}", path.display()));
        }
    }
    Ok(())
}

fn read_manifest(contents: &Path) -> Result<Manifest> {
    let bytes = plain_file(&contents.join("Resources/agent-bundle.json"))?;
    let manifest: Manifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    validate_manifest(&manifest)?;
    Ok(manifest)
}

fn validate_manifest(manifest: &Manifest) -> Result<()> {
    let is_digest = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if manifest.format_version != 1
        || Version::parse(&manifest.version).is_err()
        || !matches!(manifest.arch.as_str(), "arm64" | "x86_64")
        || !is_digest(&manifest.cli_sha256)
        || !manifest.skill_files.values().all(|value| is_digest(value))
        || manifest.skill_sha256 != skill_digest(&manifest.skill_files)
        || SKILL_FILES
            .iter()
            .any(|name| !manifest.skill_files.contains_key(*name))
        || manifest.skill_files.keys().any(|name| {
            Path::new(name)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        })
    {
        return Err("Invalid agent bundle manifest".into());
    }
    Ok(())
}

pub struct Installer {
    home: PathBuf,
    app_contents: PathBuf,
    allowed_roots: Vec<PathBuf>,
    app_version: String,
    enabled: bool,
    validated: RefCell<Option<Provider>>,
    validation_failure: Option<PathBuf>,
    last_error: Option<String>,
    #[cfg(test)]
    interrupt: Option<u8>,
    #[cfg(test)]
    link_failure: Option<PathBuf>,
    #[cfg(test)]
    link_failure_after_rename: bool,
}

impl Installer {
    pub fn new(
        home: PathBuf,
        app_contents: PathBuf,
        allowed_roots: Vec<PathBuf>,
        app_version: String,
        enabled: bool,
    ) -> Self {
        Self {
            home,
            app_contents,
            allowed_roots,
            app_version,
            enabled,
            validated: RefCell::new(None),
            validation_failure: None,
            last_error: None,
            #[cfg(test)]
            interrupt: None,
            #[cfg(test)]
            link_failure: None,
            #[cfg(test)]
            link_failure_after_rename: false,
        }
    }

    fn base(&self) -> PathBuf {
        self.home.join(".local/share/wealthlog")
    }

    fn current(&self) -> PathBuf {
        self.base().join("current")
    }

    fn record_path(&self) -> PathBuf {
        self.base().join("install.json")
    }

    fn path(&self, tool: Tool) -> PathBuf {
        self.home.join(match tool {
            Tool::Cli => ".local/bin/wealthlog",
            Tool::Skill => ".agents/skills/wealthlog",
        })
    }

    fn target(&self, tool: Tool) -> PathBuf {
        self.current().join(match tool {
            Tool::Cli => "Helpers/wealthlog",
            Tool::Skill => "Resources/skills/wealthlog",
        })
    }

    fn permitted(&self, contents: &Path) -> Result<PathBuf> {
        let canonical = fs::canonicalize(contents).map_err(|e| e.to_string())?;
        let app = canonical.parent().ok_or("Missing application parent")?;
        if canonical.file_name().and_then(|n| n.to_str()) != Some("Contents")
            || app.extension().and_then(|n| n.to_str()) != Some("app")
            || !self.allowed_roots.iter().any(|root| {
                fs::canonicalize(root)
                    .map(|root| app.starts_with(&root) && app != root)
                    .unwrap_or(false)
            })
        {
            return Err("Application must be inside /Applications or ~/Applications".into());
        }
        Ok(canonical)
    }

    fn validate(&mut self, contents: &Path) -> Result<Provider> {
        self.validation_failure = Some(contents.to_path_buf());
        if self
            .validated
            .get_mut()
            .as_ref()
            .is_some_and(|p| p.contents == contents)
        {
            *self.validated.get_mut() = None;
        }
        let contents = self.permitted(contents)?;
        let manifest = read_manifest(&contents)?;
        let cli = contents.join("Helpers/wealthlog");
        if hash(&plain_file(&cli)?) != manifest.cli_sha256
            || fs::metadata(&cli)
                .map_err(|e| e.to_string())?
                .permissions()
                .mode()
                & 0o111
                == 0
        {
            return Err("Bundled CLI digest or executable permission mismatch".into());
        }
        let skill = contents.join("Resources/skills/wealthlog");
        let meta = fs::symlink_metadata(&skill).map_err(|e| e.to_string())?;
        if !meta.is_dir() {
            return Err("Bundled skill must be a directory".into());
        }
        let mut files = BTreeMap::new();
        collect_files(&skill, &skill, &mut files)?;
        if files != manifest.skill_files
            || String::from_utf8(plain_file(&skill.join("VERSION"))?)
                .map_err(|e| e.to_string())?
                .trim()
                != manifest.version
        {
            return Err("Bundled skill is incomplete or its digest/version differs".into());
        }
        let provider = Provider { contents, manifest };
        *self.validated.get_mut() = Some(provider.clone());
        self.validation_failure = None;
        Ok(provider)
    }

    fn candidate(&mut self) -> Result<Provider> {
        let provider = self.validate(&self.app_contents.clone())?;
        let arch = if cfg!(target_arch = "aarch64") {
            "arm64"
        } else {
            "x86_64"
        };
        if provider.manifest.version != self.app_version || provider.manifest.arch != arch {
            *self.validated.get_mut() = None;
            self.validation_failure = Some(provider.contents);
            return Err("Bundle version or architecture does not match this application".into());
        }
        Ok(provider)
    }

    fn existing_provider(&mut self, recorded: &Provider) -> Result<Provider> {
        let provider = self.validate(&recorded.contents)?;
        if provider.contents != recorded.contents {
            return Err(
                "Provider resolves to a different application; explicit repair is required".into(),
            );
        }
        Ok(provider)
    }

    fn load(&self) -> Result<Option<Record>> {
        if object(&self.record_path())? == Object::Missing {
            return Ok(None);
        }
        let record: Record =
            serde_json::from_slice(&plain_file(&self.record_path())?).map_err(|e| e.to_string())?;
        if record.format_version != 1
            || validate_manifest(&record.provider.manifest).is_err()
            || !record.provider.contents.is_absolute()
            || [&record.cli, &record.skill].iter().any(|entry| {
                !matches!(entry.phase.as_str(), "idle" | "installing" | "removing")
                    || !matches!(
                        entry.result.as_str(),
                        "absent" | "installed" | "removed" | "failed"
                    )
            })
            || record.pending.as_ref().is_some_and(|changes| {
                changes.is_empty()
                    || changes.iter().enumerate().any(|(index, c)| {
                        changes[..index]
                            .iter()
                            .any(|previous| previous.path == c.path)
                    })
                    || changes.iter().any(|c| {
                        if c.path == self.current() {
                            c.new != record.provider.contents
                        } else {
                            ![Tool::Cli, Tool::Skill].iter().any(|t| {
                                c.path == self.path(*t)
                                    && c.new == self.target(*t)
                                    && c.old.as_ref().is_none_or(|old| old == &self.target(*t))
                            })
                        }
                    })
            })
        {
            return Err("Invalid installation record".into());
        }
        Ok(Some(record))
    }

    fn ensure_dir(path: &Path, mode: u32) -> Result<()> {
        match fs::metadata(path) {
            Ok(meta) if meta.is_dir() => {
                if meta.permissions().mode() & 0o222 == 0 {
                    return Err(format!("Directory is not writable: {}", path.display()));
                }
                Ok(())
            }
            Ok(_) => Err(format!("Parent is not a directory: {}", path.display())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                if let Some(parent) = path.parent() {
                    Self::ensure_dir(parent, 0o755)?;
                }
                fs::DirBuilder::new()
                    .mode(mode)
                    .create(path)
                    .and_then(|_| fs::set_permissions(path, fs::Permissions::from_mode(mode)))
                    .map_err(|e| format!("Cannot create {}: {e}", path.display()))
            }
            Err(e) => Err(format!("Cannot inspect {}: {e}", path.display())),
        }
    }

    fn prepare_endpoint_parent(&self, tool: Tool) -> Result<()> {
        let path = self.path(tool);
        let parent = path.parent().ok_or("Missing endpoint parent")?;
        Self::ensure_dir(parent, 0o755)?;
        // Mode bits do not account for ownership, ACLs or a read-only mount.
        // Exercise creation/deletion as this user without touching the endpoint.
        let probe = parent.join(format!(".wealthlog-probe-{}", uuid::Uuid::new_v4()));
        symlink(self.target(tool), &probe)
            .map_err(|e| format!("Cannot create a link in {}: {e}", parent.display()))?;
        fs::remove_file(&probe).map_err(|e| format!("Cannot remove probe {}: {e}", probe.display()))
    }

    fn lock(&self) -> Result<File> {
        Self::ensure_dir(&self.home.join(".local"), 0o755)?;
        Self::ensure_dir(&self.home.join(".local/share"), 0o755)?;
        match object(&self.base())? {
            Object::Missing => Self::ensure_dir(&self.base(), 0o700)?,
            Object::Other if fs::symlink_metadata(self.base()).is_ok_and(|m| m.is_dir()) => {
                Self::ensure_dir(&self.base(), 0o700)?;
            }
            _ => return Err("Managed installation directory is not a real directory".into()),
        }
        let path = self.base().join("install.lock");
        if !matches!(object(&path)?, Object::Missing | Object::Other)
            || fs::symlink_metadata(&path).is_ok_and(|m| !m.is_file())
        {
            return Err("Installation lock is not a regular file".into());
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(path)
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|e| format!("Installation lock busy: {e}"))?;
        Ok(lock)
    }

    fn save(&self, record: &Record) -> Result<()> {
        if !matches!(
            object(&self.record_path())?,
            Object::Missing | Object::Other
        ) || fs::symlink_metadata(self.record_path()).is_ok_and(|m| !m.is_file())
        {
            return Err("Installation record is not a regular file".into());
        }
        let temporary = self
            .base()
            .join(format!(".record-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(|e| e.to_string())?;
            file.write_all(&serde_json::to_vec_pretty(record).map_err(|e| e.to_string())?)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            fs::rename(&temporary, self.record_path()).map_err(|e| e.to_string())?;
            File::open(self.base())
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    fn link(&self, change: &Change) -> Result<()> {
        #[cfg(test)]
        if self.link_failure.as_ref() == Some(&change.path) && !self.link_failure_after_rename {
            return Err(format!(
                "Cannot publish {}: permission denied",
                change.path.display()
            ));
        }
        let expected = change
            .old
            .clone()
            .map(Object::Link)
            .unwrap_or(Object::Missing);
        if object(&change.path)? != expected {
            return Err(format!("Ownership changed: {}", change.path.display()));
        }
        let parent = change.path.parent().ok_or("Missing link parent")?;
        Self::ensure_dir(parent, 0o755)?;
        let temporary = parent.join(format!(".wealthlog-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            symlink(&change.new, &temporary).map_err(|e| e.to_string())?;
            if object(&change.path)? != expected {
                return Err(format!("Ownership changed: {}", change.path.display()));
            }
            fs::rename(&temporary, &change.path).map_err(|e| e.to_string())?;
            #[cfg(test)]
            if self.link_failure.as_ref() == Some(&change.path) && self.link_failure_after_rename {
                return Err("Directory sync failed after renaming the link".into());
            }
            File::open(parent)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        })();
        let _ = fs::remove_file(temporary);
        result
    }

    fn checkpoint(&self, _stage: u8) -> Result<()> {
        #[cfg(test)]
        if self.interrupt == Some(_stage) {
            return Err(format!("Interrupted at stage {_stage}"));
        }
        Ok(())
    }

    fn publish(
        &mut self,
        record: &mut Record,
        provider: Provider,
        changes: Vec<Change>,
    ) -> Result<()> {
        // Revalidate immediately before publishing; polling never walks resources.
        let actual = self.validate(&provider.contents)?;
        if actual != provider {
            return Err("Bundle changed before publication".into());
        }
        record.provider = provider;
        let mut failed_tools = Vec::new();
        let mut unresolved = Vec::new();
        let mut failures = Vec::new();
        if !changes.is_empty() {
            for tool in [Tool::Cli, Tool::Skill] {
                if record.entry(tool).desired && record.entry(tool).result != "failed" {
                    record.entry_mut(tool).phase = "installing".into();
                }
            }
            record.pending = Some(changes.clone());
            self.save(record)?;
            self.checkpoint(1)?;
            for change in &changes {
                if let Err(error) = self.link(change) {
                    for tool in [Tool::Cli, Tool::Skill] {
                        if change.path == self.current() || change.path == self.path(tool) {
                            record.entry_mut(tool).result = "failed".into();
                            record.entry_mut(tool).error = Some(error.clone());
                            record.entry_mut(tool).phase = "idle".into();
                            failed_tools.push(tool);
                        }
                    }
                    if change.path == self.current() {
                        self.save(record)?;
                        return Err(error);
                    }
                    let old = change
                        .old
                        .clone()
                        .map(Object::Link)
                        .unwrap_or(Object::Missing);
                    if object(&change.path).is_ok_and(|actual| actual == old) {
                        // A confirmed unchanged endpoint has a completed failure,
                        // not an interrupted operation. Persist this before continuing.
                        if let Some(pending) = &mut record.pending {
                            pending.retain(|c| c.path != change.path);
                            if pending.is_empty() {
                                record.pending = None;
                            }
                        }
                    } else {
                        // A changed/unknown path still requires ownership recovery.
                        unresolved.push(change.clone());
                    }
                    failures.push(error);
                    self.save(record)?;
                }
            }
            self.checkpoint(2)?;
        }
        record.pending = (!unresolved.is_empty()).then_some(unresolved);
        for tool in [Tool::Cli, Tool::Skill] {
            if failed_tools.contains(&tool) {
                continue;
            }
            if object(&self.path(tool)).is_ok_and(|o| o == Object::Link(self.target(tool))) {
                let entry = record.entry_mut(tool);
                if entry.desired {
                    entry.installed = true;
                    entry.phase = "idle".into();
                    entry.result = "installed".into();
                    entry.error = None;
                }
            }
        }
        self.save(record)?;
        self.checkpoint(3)?;
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn ownership(&self, record: &Record) -> Result<()> {
        if let Some(changes) = &record.pending {
            for change in changes {
                let actual = object(&change.path)?;
                let old = change
                    .old
                    .clone()
                    .map(Object::Link)
                    .unwrap_or(Object::Missing);
                if actual != old && actual != Object::Link(change.new.clone()) {
                    return Err(format!("Ownership conflict: {}", change.path.display()));
                }
            }
            // Unaffected entries also remain protected while a transaction is pending.
        }
        let current_pending = record
            .pending
            .as_ref()
            .is_some_and(|changes| changes.iter().any(|c| c.path == self.current()));
        if !current_pending {
            match object(&self.current())? {
                Object::Missing => {}
                Object::Link(target) if target == record.provider.contents => {}
                _ => return Err("Ownership conflict: current".into()),
            }
        }
        for tool in [Tool::Cli, Tool::Skill] {
            let pending = record
                .pending
                .as_ref()
                .is_some_and(|changes| changes.iter().any(|c| c.path == self.path(tool)));
            if !pending {
                match object(&self.path(tool)) {
                    Err(_) => {}
                    Ok(Object::Missing) => {}
                    Ok(Object::Link(target)) if target == self.target(tool) => {}
                    _ => return Err(format!("Ownership conflict: {tool:?}")),
                }
            }
        }
        Ok(())
    }

    fn choose(&self, old: &Provider, new: &Provider, explicit: bool) -> Result<bool> {
        if old.contents == new.contents {
            return Ok(true);
        }
        let old_version = Version::parse(&old.manifest.version).map_err(|e| e.to_string())?;
        let new_version = Version::parse(&new.manifest.version).map_err(|e| e.to_string())?;
        match new_version.cmp_precedence(&old_version) {
            std::cmp::Ordering::Greater => return Ok(true),
            std::cmp::Ordering::Less => return Ok(false),
            std::cmp::Ordering::Equal => {}
        }
        if explicit {
            return Ok(true);
        }
        // A dangling old provider may be repaired only by an identical bundle.
        Ok(object(&old.contents)? == Object::Missing && old.manifest == new.manifest)
    }

    fn any_existing(&self) -> Result<bool> {
        if object(&self.current())? != Object::Missing {
            return Ok(true);
        }
        for path in [self.path(Tool::Cli), self.path(Tool::Skill)] {
            if object(&path).is_ok_and(|o| o != Object::Missing) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn observe_removals(&self, record: &mut Record) {
        if record.pending.is_some() {
            return;
        }
        for tool in [Tool::Cli, Tool::Skill] {
            let actual = object(&self.path(tool));
            let entry = record.entry_mut(tool);
            match actual {
                Ok(Object::Missing) if entry.installed => {
                    entry.desired = false;
                    entry.result = "removed".into();
                    entry.phase = "idle".into();
                    entry.error = None;
                }
                Err(error) => {
                    entry.result = "failed".into();
                    entry.error = Some(error);
                }
                _ => {}
            }
        }
    }

    fn run_startup(&mut self) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        self.permitted(&self.app_contents)?;
        let _lock = self.lock()?;
        let candidate = self.candidate()?;
        let mut record = match self.load()? {
            Some(record) => record,
            None => {
                if self.any_existing()? {
                    return Err(
                        "Installation record needs repair; existing paths were preserved".into(),
                    );
                }
                Record {
                    format_version: 1,
                    provider: candidate.clone(),
                    cli: Entry::default(),
                    skill: Entry::default(),
                    pending: None,
                }
            }
        };
        self.ownership(&record)?;
        if record.pending.is_some() {
            return Err("Interrupted installation needs repair".into());
        }
        // A missing endpoint is an opt-out; dangling endpoints still exist.
        self.observe_removals(&mut record);
        self.save(&record)?;
        if !record.cli.desired && !record.skill.desired {
            return Ok(());
        }
        // A replacement at the owning path may not have launched yet. Compare
        // against its validated current version, not a stale recorded version.
        if candidate.contents != record.provider.contents
            && fs::metadata(&record.provider.contents).is_ok()
        {
            record.provider = self.existing_provider(&record.provider)?;
        }
        let provider = if self.choose(&record.provider, &candidate, false)? {
            candidate
        } else {
            self.existing_provider(&record.provider)?
        };
        let mut changes = Vec::new();
        match object(&self.current())? {
            Object::Missing => changes.push(Change {
                path: self.current(),
                old: None,
                new: provider.contents.clone(),
            }),
            Object::Link(old) if old != provider.contents => changes.push(Change {
                path: self.current(),
                old: Some(old),
                new: provider.contents.clone(),
            }),
            _ => {}
        }
        // Path failures belong to one endpoint and do not suppress the other.
        for tool in [Tool::Cli, Tool::Skill] {
            if !record.entry(tool).desired {
                continue;
            }
            // Probe only when a link must be created. A correct managed link
            // needs no parent write access and must not appear as a failure.
            let attempt = match object(&self.path(tool)) {
                Ok(Object::Missing) => self.prepare_endpoint_parent(tool).map(|()| {
                    Some(Change {
                        path: self.path(tool),
                        old: None,
                        new: self.target(tool),
                    })
                }),
                Ok(Object::Link(target)) if target == self.target(tool) => Ok(None),
                Ok(_) => Err("Endpoint is managed by the user".to_string()),
                Err(error) => Err(error),
            };
            match attempt {
                Ok(Some(change)) => changes.push(change),
                Ok(None) => {
                    let entry = record.entry_mut(tool);
                    entry.installed = true;
                    entry.phase = "idle".into();
                    entry.result = "installed".into();
                    entry.error = None;
                }
                Err(error) => {
                    let entry = record.entry_mut(tool);
                    entry.result = "failed".into();
                    entry.error = Some(error);
                }
            }
        }
        if changes.is_empty() {
            record.provider = provider;
            self.save(&record)
        } else {
            self.publish(&mut record, provider, changes)
        }
    }

    pub fn startup(&mut self) {
        *self.validated.get_mut() = None;
        self.validation_failure = None;
        self.last_error = self.run_startup().err();
    }

    fn repair_record(&mut self) -> Result<Record> {
        let current = object(&self.current())?;
        let provider = match current {
            Object::Link(target) => self.validate(&target)?,
            Object::Missing => self.candidate()?,
            _ => return Err("Ownership conflict: current is not a managed link".into()),
        };
        let mut record = Record {
            format_version: 1,
            provider,
            cli: Entry::default(),
            skill: Entry::default(),
            pending: None,
        };
        for tool in [Tool::Cli, Tool::Skill] {
            let installed = match object(&self.path(tool))? {
                Object::Missing => false,
                Object::Link(target) if target == self.target(tool) => true,
                _ => return Err("Cannot adopt external endpoint".into()),
            };
            let entry = record.entry_mut(tool);
            entry.desired = installed;
            entry.installed = installed;
            entry.result = if installed { "installed" } else { "removed" }.into();
        }
        Ok(record)
    }

    fn run_action(&mut self, action: Action, tool: Option<Tool>) -> Result<()> {
        if !self.enabled {
            return Err("Agent tool installation is unsupported in this environment".into());
        }
        self.permitted(&self.app_contents)?;
        let _lock = self.lock()?;
        let mut record = match self.load() {
            Ok(Some(record)) => record,
            Ok(None) | Err(_) if matches!(action, Action::Repair) => self.repair_record()?,
            _ => return Err("Repair the installation record before changing tools".into()),
        };
        self.observe_removals(&mut record);
        if matches!(action, Action::Remove | Action::KeepRemoved) {
            let tool = tool.ok_or("Choose CLI or skill")?;
            let entry = record.entry_mut(tool);
            entry.desired = false;
            entry.phase = "removing".into();
            entry.result = "removed".into();
            entry.error = None;
            self.save(&record)?;
            let removal: Result<()> = (|| {
                if object(&self.path(tool))? == Object::Link(self.target(tool)) {
                    fs::remove_file(self.path(tool)).map_err(|e| e.to_string())?;
                }
                Ok(())
            })();
            let entry = record.entry_mut(tool);
            entry.phase = "idle".into();
            if let Err(error) = &removal {
                entry.result = "failed".into();
                entry.error = Some(error.clone());
            }
            self.save(&record)?;
            return removal;
        }
        self.ownership(&record)?;
        if record.pending.is_some() && !matches!(action, Action::Repair) {
            return Err("Interrupted installation needs repair first".into());
        }
        let candidate = self.candidate()?;
        if !matches!(action, Action::Repair)
            && candidate.contents != record.provider.contents
            && fs::metadata(&record.provider.contents).is_ok()
        {
            match self.existing_provider(&record.provider) {
                Ok(provider) => record.provider = provider,
                Err(error) if !matches!(action, Action::UseCopy) => return Err(error),
                Err(_) => {}
            }
        }
        let provider = if matches!(action, Action::Repair) {
            let current_exists = match object(&self.current())? {
                Object::Link(target) => fs::metadata(target).is_ok_and(|m| m.is_dir()),
                _ => false,
            };
            if current_exists && fs::metadata(&record.provider.contents).is_ok_and(|m| m.is_dir()) {
                self.existing_provider(&record.provider)?
            } else if self.choose(&record.provider, &candidate, false)? {
                candidate
            } else {
                return Err(
                    "This copy cannot repair the missing provider; use an eligible copy".into(),
                );
            }
        } else if self.choose(
            &record.provider,
            &candidate,
            matches!(action, Action::UseCopy),
        )? {
            candidate
        } else {
            self.existing_provider(&record.provider)?
        };
        if matches!(action, Action::UseCopy)
            && provider.contents != self.permitted(&self.app_contents)?
        {
            return Err("An older copy cannot take over tools from another path".into());
        }
        if matches!(action, Action::Reinstall | Action::Restore) {
            let tool = tool.ok_or("Choose CLI or skill")?;
            record.entry_mut(tool).desired = true;
            record.entry_mut(tool).error = None;
            if object(&self.path(tool))? == Object::Missing {
                record.entry_mut(tool).installed = false;
            }
        }
        let mut changes = Vec::new();
        let old = match object(&self.current())? {
            Object::Missing => None,
            Object::Link(target) => Some(target),
            _ => return Err("Ownership conflict: current".into()),
        };
        if old.as_ref() != Some(&provider.contents) {
            changes.push(Change {
                path: self.current(),
                old,
                new: provider.contents.clone(),
            });
        }
        for entry_tool in [Tool::Cli, Tool::Skill] {
            if !record.entry(entry_tool).desired
                || (matches!(action, Action::Reinstall | Action::Restore)
                    && tool != Some(entry_tool))
            {
                continue;
            }
            match object(&self.path(entry_tool)) {
                Ok(Object::Missing) => {
                    let path = self.path(entry_tool);
                    if let Err(error) = self.prepare_endpoint_parent(entry_tool) {
                        let entry = record.entry_mut(entry_tool);
                        entry.result = "failed".into();
                        entry.error = Some(error.clone());
                        self.save(&record)?;
                        if tool == Some(entry_tool) {
                            return Err(error);
                        }
                        continue;
                    }
                    changes.push(Change {
                        path,
                        old: None,
                        new: self.target(entry_tool),
                    });
                }
                Err(error) => {
                    record.entry_mut(entry_tool).result = "failed".into();
                    record.entry_mut(entry_tool).error = Some(error);
                }
                _ => {}
            }
        }
        // An explicit repair acknowledges and completes a valid interrupted journal.
        self.publish(&mut record, provider, changes)
    }

    pub fn action(&mut self, action: Action, tool: Option<Tool>) -> Result<Status> {
        match self.run_action(action, tool) {
            Ok(()) => {
                self.last_error = None;
                Ok(self.status())
            }
            Err(error) => {
                self.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub fn status(&self) -> Status {
        let location_supported = self.enabled && self.permitted(&self.app_contents).is_ok();
        let loaded = self.load();
        let record = loaded.as_ref().ok().and_then(|r| r.as_ref());
        let ownership = record.map(|r| self.ownership(r));
        let interrupted = record.is_some_and(|r| r.pending.is_some());
        let no_record_existing = record.is_none() && self.any_existing().unwrap_or(true);
        let current = object(&self.current());
        let provider_matches = record.is_some_and(|r| {
            current
                .as_ref()
                .is_ok_and(|o| o == &Object::Link(r.provider.contents.clone()))
                && fs::metadata(&r.provider.contents).is_ok_and(|meta| meta.is_dir())
        });
        let invalidated = self.validated.borrow().as_ref().is_some_and(|cached| {
            !provider_matches || record.is_none_or(|r| cached != &r.provider)
        });
        if invalidated {
            self.validated.replace(None);
        }
        let verified = record.is_some_and(|r| {
            provider_matches && self.validated.borrow().as_ref() == Some(&r.provider)
        });
        let status_for = |tool: Tool| {
            let entry = record.map(|r| r.entry(tool));
            let actual = object(&self.path(tool));
            let desired = entry.is_none_or(|e| e.desired);
            let error = actual
                .as_ref()
                .err()
                .cloned()
                .or_else(|| entry.and_then(|e| e.error.clone()));
            let state = if !self.enabled {
                "unsupported"
            } else if !location_supported {
                "unsupported_location"
            } else if actual.is_err() {
                "failed"
            } else if record.is_none()
                && actual.as_ref().is_ok_and(|o| {
                    matches!(o, Object::Other)
                        || matches!(o, Object::Link(target) if target != &self.target(tool))
                })
            {
                "user_managed"
            } else if loaded.is_err() || no_record_existing {
                "needs_repair"
            } else if ownership.as_ref().is_some_and(|r| r.is_err()) {
                if ownership
                    .as_ref()
                    .is_some_and(|r| r.as_ref().err().is_some_and(|e| e.starts_with("Cannot")))
                {
                    "failed"
                } else {
                    "conflict"
                }
            } else if interrupted {
                "needs_repair"
            } else if entry.is_some_and(|e| e.result == "failed")
                || (record.is_none() && self.last_error.is_some())
                || record
                    .is_some_and(|r| self.validation_failure.as_ref() == Some(&r.provider.contents))
            {
                "failed"
            } else if !desired {
                if actual.as_ref().is_ok_and(|a| *a == Object::Missing) {
                    "removed"
                } else {
                    "state_conflict"
                }
            } else if actual.as_ref().is_ok_and(|a| *a == Object::Missing) {
                if entry.is_some_and(|e| e.installed) {
                    "removed"
                } else {
                    "installing"
                }
            } else if !provider_matches {
                "needs_repair"
            } else if !verified {
                "pending_validation"
            } else if record.is_some_and(|r| {
                self.permitted(&self.app_contents).ok().as_ref() == Some(&r.provider.contents)
            }) {
                "provided_here"
            } else {
                "provided_elsewhere"
            };
            ToolStatus {
                state: state.into(),
                path: self.path(tool),
                desired,
                error,
            }
        };
        let error = self
            .last_error
            .clone()
            .or_else(|| loaded.as_ref().err().cloned())
            .or_else(|| ownership.as_ref().and_then(|r| r.as_ref().err().cloned()));
        let cli = status_for(Tool::Cli);
        let skill = status_for(Tool::Skill);
        let can_repair = [&cli, &skill].iter().any(|entry| {
            matches!(
                entry.state.as_str(),
                "needs_repair" | "pending_validation" | "conflict" | "user_managed"
            )
        });
        let can_use_copy = record.is_some_and(|r| {
            r.pending.is_none()
                && self
                    .permitted(&self.app_contents)
                    .is_ok_and(|path| path != r.provider.contents)
                && Version::parse(&self.app_version)
                    .ok()
                    .zip(Version::parse(&r.provider.manifest.version).ok())
                    .is_some_and(|(new, old)| new.cmp_precedence(&old) != std::cmp::Ordering::Less)
        });
        Status {
            supported: self.enabled,
            location_supported,
            cli,
            skill,
            provider_app: record.and_then(|r| r.provider.contents.parent().map(Path::to_path_buf)),
            current_app: self
                .permitted(&self.app_contents)
                .ok()
                .and_then(|p| p.parent().map(Path::to_path_buf)),
            repair_app: if current.as_ref().is_ok_and(|o| {
                matches!(o, Object::Link(target) if fs::metadata(target).is_ok_and(|m| m.is_dir()))
            }) {
                if let Some(record) = record.filter(|r| r.pending.is_some()) {
                    if fs::metadata(&record.provider.contents).is_ok_and(|m| m.is_dir()) {
                        record.provider.contents.parent().map(Path::to_path_buf)
                    } else {
                        self.permitted(&self.app_contents).ok().and_then(|p| p.parent().map(Path::to_path_buf))
                    }
                } else if let Ok(Object::Link(target)) = &current {
                    target.parent().map(Path::to_path_buf)
                } else {
                    None
                }
            } else {
                self.permitted(&self.app_contents).ok().and_then(|p| p.parent().map(Path::to_path_buf))
            },
            version: record.map(|r| r.provider.manifest.version.clone()),
            config: serde_json::json!({
                "cli": self.path(Tool::Cli),
                "appId": "local.kanelogger.wealthlog",
                "ledgerId": "REPLACE_WITH_VERIFIED_LEDGER_ID",
                "tokenFile": self.home.join(".config/wealthlog-agent/token"),
                "timezone": "Asia/Shanghai",
                "stateDir": self.home.join("Wealthlog-Agent/.wealthlog-agent-state"),
            }),
            error,
            can_repair: self.enabled && location_supported && can_repair,
            can_use_copy: self.enabled && location_supported && can_use_copy,
        }
    }
}

#[cfg(test)]
mod tests;
