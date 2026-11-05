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
    adjustment_nand: String,
    card_sha256: Option<String>,
    procedure: String,
}

impl BootIdentity {
    pub fn new(document: &[u8], update: &[u8], boot_rom: &[u8], adjustment: &[u8], card_sha256: Option<&str>, procedure: &[u8]) -> Self {
        Self {
            schema: 2,
            model: env!("XM1_CHECKPOINT_MODEL_ID"),
            run_document: sha256(document),
            installed_dat: sha256(update),
            boot_rom: sha256(boot_rom),
            adjustment_nand: sha256(adjustment),
            card_sha256: card_sha256.map(str::to_owned),
            procedure: sha256(procedure),
        }
    }

    pub fn key(&self) -> CheckpointKey {
        CheckpointKey(sha256(&serde_json::to_vec(self).expect("boot identity serialization")))
    }

    pub fn document(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("boot identity serialization")
    }
}

pub fn initialized_reset(mut machine: crate::machine::Machine, identity: &BootIdentity, root: &std::path::Path) -> Result<crate::machine::Machine, String> {
    const RECORD: &str = "initialized-reset.json";
    let name = format!("reset-{}", identity.key());
    let directory = root.join(&name);
    if directory.exists() {
        let recorded: serde_json::Value = serde_json::from_slice(&crate::bridge::read(&directory.join(RECORD))?)
            .map_err(|error| format!("initialized-reset identity: {error}"))?;
        if recorded != identity.document() { return Err("initialized-reset checkpoint identity differs".into()); }
        let snapshot = machine.session.import_snapshot(&crate::bridge::read(&directory.join(crate::bridge::SNAPSHOT))?)?;
        machine.disarm()?;
        machine.session.restore(&snapshot)?;
        machine.rearm()?;
    }
    if machine.session.board.instruction_counts().iter().any(|count| *count != 0) {
        return Err("initialized-reset checkpoint omits startup evidence".into());
    }
    if !directory.exists() {
        crate::bridge::save_checkpoint(&mut machine, root, &name, RECORD, &identity.document())?;
    }
    Ok(machine)
}
