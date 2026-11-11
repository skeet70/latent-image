//! Content-addressed boot state, independent of the observer executable.
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
        }
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
