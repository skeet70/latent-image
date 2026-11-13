//! Content-addressed guest state at declared execution boundaries.
use crate::bridge::sha256;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct CheckpointKey(String);

impl CheckpointKey {
    pub fn as_str(&self) -> &str { &self.0 }
}

impl std::fmt::Display for CheckpointKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(&self.0) }
}

#[derive(Serialize)]
pub struct BootIdentity {
    schema: u32,
    model: &'static str,
    run_document: String,
    installed_dat: String,
    boot_rom: String,
    adjustment_nand: Option<String>,
    record_nand: Option<String>,
    committed_nand: Option<String>,
    source_kind: NandSourceKind,
    card_sha256: Option<String>,
    procedure: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    emulator_binary: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
enum NandSourceKind { InitialSeeded, FullCommitted }

impl BootIdentity {
    pub fn new(document: &[u8], update: &[u8], boot_rom: &[u8], adjustment: &[u8], card_sha256: Option<&str>, procedure: &[u8]) -> Self {
        Self {
            schema: 3,
            model: env!("XM1_CHECKPOINT_MODEL_ID"),
            run_document: sha256(document),
            installed_dat: sha256(update),
            boot_rom: sha256(boot_rom),
            adjustment_nand: Some(sha256(adjustment)),
            record_nand: None,
            committed_nand: None,
            source_kind: NandSourceKind::InitialSeeded,
            card_sha256: card_sha256.map(str::to_owned),
            procedure: sha256(procedure),
            emulator_binary: None,
        }
    }

    pub fn with_emulator_binary(mut self, executable: &[u8]) -> Self {
        self.emulator_binary = Some(sha256(executable));
        self
    }

    pub fn with_record_nand(mut self, records: Option<&[u8]>) -> Self {
        self.record_nand = records.map(sha256);
        self
    }

    pub fn with_committed_nand(mut self, committed: Option<&[u8]>) -> Self {
        self.committed_nand = committed.map(sha256);
        if committed.is_some() {
            self.adjustment_nand = None;
            self.record_nand = None;
            self.source_kind = NandSourceKind::FullCommitted;
        }
        self
    }

    pub fn key(&self) -> CheckpointKey {
        CheckpointKey(sha256(&serde_json::to_vec(&self.document()).expect("boot identity serialization")))
    }

    pub fn document(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("boot identity serialization")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CacheLookup { Hit, Miss }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResetRestore { Restored, NotRestored }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ResetCache {
    lookup: CacheLookup,
    restore: ResetRestore,
}

impl ResetCache {
    pub fn lookup(self) -> CacheLookup { self.lookup }
    pub fn restore(self) -> ResetRestore { self.restore }
}

pub struct InitializedReset {
    pub machine: crate::machine::Machine,
    pub cache: ResetCache,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetCachePolicy { ReuseOrCreate, RequireHitRestored }


pub fn initialized_reset(mut machine: crate::machine::Machine, identity: &BootIdentity, root: &std::path::Path, budget:Option<crate::bridge::CheckpointBudget>, policy: ResetCachePolicy) -> Result<InitializedReset, String> {
    const RECORD: &str = "initialized-reset.json";
    let name = format!("reset-{}", identity.key());
    let directory = root.join(&name);
    if policy == ResetCachePolicy::RequireHitRestored && !directory.is_dir() {
        return Err("AF reset requires an existing authenticated zero-instruction cache".into());
    }
    let cache;
    if directory.exists() {
        if let Some(budget)=budget {
            let size=|name|std::fs::metadata(directory.join(name)).map(|metadata|metadata.len()).map_err(|error|error.to_string());
            budget.admit(size(crate::bridge::SNAPSHOT)?,size(RECORD)?)?;
        }
        let recorded: serde_json::Value = serde_json::from_slice(&crate::bridge::read(&directory.join(RECORD))?)
            .map_err(|error| format!("initialized-reset identity: {error}"))?;
        if recorded != identity.document() { return Err("initialized-reset checkpoint identity differs".into()); }
        let snapshot = machine.session.import_snapshot(&crate::bridge::read(&directory.join(crate::bridge::SNAPSHOT))?)?;
        machine.disarm()?;
        machine.session.restore(&snapshot)?;
        machine.rearm()?;
        cache = ResetCache { lookup: CacheLookup::Hit, restore: ResetRestore::Restored };
    } else {
        cache = ResetCache { lookup: CacheLookup::Miss, restore: ResetRestore::NotRestored };
    }
    if machine.session.board.instruction_counts().iter().any(|count| *count != 0) {
        return Err("initialized-reset checkpoint omits startup evidence".into());
    }
    if policy == ResetCachePolicy::RequireHitRestored && machine.session.board.now() != 0 {
        return Err("AF reset cache must have zero board time".into());
    }
    if !directory.exists() {
        crate::bridge::save_checkpoint(&mut machine, root, &name, RECORD, &identity.document(), budget)?;
    }
    Ok(InitializedReset { machine, cache })
}

pub const SHARED_STORE: &str = "private-data/emulator-checkpoints";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PostBootRecord {
    pub identity: serde_json::Value,
    pub snapshot_sha256: String,
    pub instructions: u64,
}

#[derive(Serialize)]
pub struct PostBootReceipt {
    pub key: CheckpointKey,
    pub record: PostBootRecord,
    pub lookup: CacheLookup,
}

/// The producer runs without scenario observers. The receipt binds the bytes restored.
pub fn post_boot(
    machine: &mut crate::machine::Machine,
    identity: &BootIdentity,
    root: &std::path::Path,
    produce: impl FnOnce(&mut crate::machine::Machine) -> Result<(), String>,
) -> Result<PostBootReceipt, String> {
    use std::os::fd::AsRawFd;
    const RECORD: &str = "post-boot.json";
    if identity.emulator_binary.is_none() {
        return Err("post-boot checkpoint requires the emulator executable digest".into());
    }
    std::fs::create_dir_all(root).map_err(|error| error.to_string())?;
    let key = identity.key();
    let name = format!("post-boot-{key}");
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true)
        .open(root.join(".post-boot.lock")).map_err(|error| error.to_string())?;
    // The descriptor owns the lock until this function returns.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    evict_superseded(root, identity)?;
    let directory = root.join(&name);
    let (record, lookup) = if directory.exists() {
        let record: PostBootRecord = serde_json::from_slice(&crate::bridge::read(&directory.join(RECORD))?)
            .map_err(|error| format!("post-boot record: {error}"))?;
        if record.identity != identity.document() {
            return Err("post-boot checkpoint identity differs".into());
        }
        let bytes = crate::bridge::read(&directory.join(crate::bridge::SNAPSHOT))?;
        if sha256(&bytes) != record.snapshot_sha256 {
            return Err("post-boot checkpoint snapshot digest differs".into());
        }
        let snapshot = machine.session.import_snapshot(&bytes)?;
        machine.disarm()?;
        machine.session.restore(&snapshot)?;
        machine.rearm()?;
        if machine.instructions() != record.instructions || record.instructions == 0 {
            return Err("post-boot checkpoint instruction boundary differs".into());
        }
        (record, CacheLookup::Hit)
    } else {
        produce(machine)?;
        machine.require_running()?;
        if machine.instructions() == 0 {
            return Err("post-boot producer did not execute the guest".into());
        }
        machine.disarm()?;
        let snapshot = machine.session.checkpoint(None).and_then(|snapshot| snapshot.export());
        machine.rearm()?;
        let snapshot = snapshot?;
        let record = PostBootRecord {
            identity: identity.document(),
            snapshot_sha256: sha256(&snapshot),
            instructions: machine.instructions(),
        };
        let partial = root.join(format!(".partial-{name}-{}", std::process::id()));
        std::fs::create_dir(&partial).map_err(|error| error.to_string())?;
        let published = (|| -> Result<(), String> {
            crate::bridge::write(&partial.join(crate::bridge::SNAPSHOT), &snapshot)?;
            crate::bridge::write(&partial.join(RECORD), &serde_json::to_vec_pretty(&record).map_err(|error| error.to_string())?)?;
            for path in [partial.join(crate::bridge::SNAPSHOT), partial.join(RECORD), partial.clone()] {
                std::fs::File::open(path).and_then(|file| file.sync_all()).map_err(|error| error.to_string())?;
            }
            std::fs::rename(&partial, &directory).map_err(|error| error.to_string())?;
            std::fs::File::open(root).and_then(|file| file.sync_all()).map_err(|error| error.to_string())
        })();
        if published.is_err() { let _ = std::fs::remove_dir_all(&partial); }
        published?;
        (record, CacheLookup::Miss)
    };
    Ok(PostBootReceipt { key, record, lookup })
}

fn evict_superseded(root: &std::path::Path, identity: &BootIdentity) -> Result<(), String> {
    let current = identity.document();
    let mut comparable = current.clone();
    comparable.as_object_mut().expect("boot identity object").remove("emulator_binary");
    for entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !entry.file_type().map_err(|error| error.to_string())?.is_dir() { continue; }
        if name.starts_with(".partial-post-boot-") {
            std::fs::remove_dir_all(entry.path()).map_err(|error| error.to_string())?;
            continue;
        }
        if !name.starts_with("post-boot-") { continue; }
        let record_path = entry.path().join("post-boot.json");
        let Ok(metadata) = std::fs::metadata(&record_path) else { continue };
        let old = metadata.modified().ok().and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age >= std::time::Duration::from_secs(7 * 24 * 60 * 60));
        if !old { continue; }
        let Ok(bytes) = std::fs::read(record_path) else { continue };
        let Ok(mut record) = serde_json::from_slice::<PostBootRecord>(&bytes) else { continue };
        if record.identity["emulator_binary"] == current["emulator_binary"] { continue; }
        let Some(fields) = record.identity.as_object_mut() else { continue };
        fields.remove("emulator_binary");
        if record.identity == comparable {
            std::fs::remove_dir_all(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_boot_reuses_bytes_and_invalidates_changed_inputs() {
        use crate::{machine::Machine, run::{self, RunDocument, ValidatedUpdate}};
        const RUN: &[u8] = include_bytes!("../data/xm1-150.json");
        fujimod_emulator_exr2_board::register_board_builder();
        fujimod_emulator_models::register();
        let document = RunDocument::parse(RUN).unwrap();
        let update = vec![0xff; document.update_flash_offset + 2048];
        let mut rom = vec![0; document.boot_rom.bytes];
        rom[..4].copy_from_slice(&0xe280_0001u32.to_le_bytes());
        rom[4..8].copy_from_slice(&0xeaff_fffdu32.to_le_bytes());
        let root = std::env::temp_dir().join(format!("post-boot-cache-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let make_machine = || {
            let document = RunDocument::parse(RUN).unwrap();
            let mut session = run::open(&document, &ValidatedUpdate(update.clone()), &rom).unwrap();
            session.board.set_execution_policy(document.execution_policy.into()).unwrap();
            Machine::new(document, session).unwrap()
        };
        let identity = |run: &[u8], dat: &[u8], executable: &[u8]| {
            BootIdentity::new(run, dat, &rom, &[], None, b"synthetic-loop-v1").with_emulator_binary(executable)
        };
        let first_identity = identity(RUN, &update, b"emulator-one");
        let produce = |machine: &mut Machine| {
            machine.run(4);
            machine.require_running()
        };
        let mut produced = make_machine();
        let first = post_boot(&mut produced, &first_identity, &root, produce).unwrap();
        assert_eq!(first.lookup, CacheLookup::Miss);
        let mut restored = make_machine();
        let second = post_boot(&mut restored, &first_identity, &root, |_| Err("unchanged input executed producer".into())).unwrap();
        assert_eq!(second.lookup, CacheLookup::Hit);
        assert_eq!(first.record, second.record);
        assert_eq!(restored.instructions(), first.record.instructions);
        assert_eq!(crate::machine_fingerprint::fingerprint(&produced.session).unwrap(),
            crate::machine_fingerprint::fingerprint(&restored.session).unwrap());
        let register = fujimod_emulator_cpu::arm::CORE[0];
        let before = produced.session.board.read_register(0, register).unwrap();
        produced.run(8);
        restored.run(8);
        produced.require_running().unwrap();
        restored.require_running().unwrap();
        let after = produced.session.board.read_register(0, register).unwrap();
        assert!(after > before);
        assert_eq!(after, restored.session.board.read_register(0, register).unwrap());
        assert_eq!(produced.session.board.instruction_counts(), restored.session.board.instruction_counts());
        let mut changed_run = RUN.to_vec();
        changed_run.push(b'\n');
        let mut changed_update = update.clone();
        changed_update[0] ^= 1;
        for changed in [identity(&changed_run, &update, b"emulator-one"), identity(RUN, &changed_update, b"emulator-one"), identity(RUN, &update, b"emulator-two")] {
            let receipt = post_boot(&mut make_machine(), &changed, &root, produce).unwrap();
            assert_eq!(receipt.lookup, CacheLookup::Miss);
            assert_ne!(receipt.key, first.key);
        }
        let snapshot = root.join(format!("post-boot-{}", first.key)).join(crate::bridge::SNAPSHOT);
        crate::bridge::write(&snapshot, b"corrupted checkpoint").unwrap();
        assert!(post_boot(&mut make_machine(), &first_identity, &root, produce).is_err());
        let abandoned = root.join(".partial-post-boot-abandoned");
        std::fs::create_dir(&abandoned).unwrap();
        let first_directory = snapshot.parent().unwrap();
        std::fs::File::open(first_directory.join("post-boot.json")).unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH)).unwrap();
        let latest = identity(RUN, &update, b"emulator-three");
        post_boot(&mut make_machine(), &latest, &root, produce).unwrap();
        assert!(!abandoned.exists());
        assert!(!first_directory.exists());
        assert!(root.join(format!("post-boot-{}", latest.key())).is_dir());
        std::fs::remove_dir_all(root).unwrap();
    }
}
