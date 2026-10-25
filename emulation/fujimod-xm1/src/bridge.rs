//! Pieces the JSON-lines bridges share: their inputs, JSON output and
//! session checkpoints saved as FujiMod snapshots.

use crate::adjustment::AdjustmentNand;
use crate::guest::MenuState;
use crate::machine::Machine;
use crate::run::{self, RunDocument, ValidatedUpdate};
use crate::sd_host::{self, RetainedCard};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const SNAPSHOT: &str = "session.fujimod";

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    std::fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn emit(value: &Value) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    writeln!(out, "{value}").and_then(|()| out.flush()).map_err(|e| e.to_string())
}

pub fn menu_value(menu: Option<MenuState>) -> Value {
    match menu {
        Some(menu) => json!({"menu_id": menu.menu_id, "setting_id": menu.setting_id, "cursor": menu.cursor}),
        None => json!({"menu_id": null, "setting_id": null, "cursor": null}),
    }
}

/// Everything needed to build the same unexecuted session again.
pub struct Inputs {
    pub document_bytes: Vec<u8>,
    pub update: Vec<u8>,
    pub boot_rom: Vec<u8>,
    pub adjustment: AdjustmentNand,
    /// The card inserted before the first instruction; absent leaves the slot empty.
    pub card: Option<RetainedCard>,
}

impl Inputs {
    pub fn machine(&self) -> Result<Machine, String> {
        let document = RunDocument::parse(&self.document_bytes)?;
        let mut session = run::open(&document, &ValidatedUpdate(self.update.clone()), &self.boot_rom)?;
        session.board.set_execution_policy(document.execution_policy.into())?;
        self.adjustment.apply(&mut session, &document.nand)?;
        let mut machine = Machine::new(document, session)?;
        if let Some(card) = &self.card {
            machine.insert_card(card.clone())?;
        }
        Ok(machine)
    }
}

/// Loads the raw image at `path` as a card of the document's SD host.
pub fn load_card(document_bytes: &[u8], path: &Path) -> Result<RetainedCard, String> {
    let document = RunDocument::parse(document_bytes)?;
    let (sectors, max_dirty_sectors) = sd_host::card_geometry(&document.sd_host()?)?;
    sd_host::load_image(path, sectors, max_dirty_sectors)
}

/// Saves the machine's session under `root/name` through a partial directory,
/// replacing an existing checkpoint of the same name.
pub fn save_checkpoint(machine: &mut Machine, root: &Path, name: &str, record_file: &str, record: &Value) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let partial = root.join(format!(".partial-{name}-{}", std::process::id()));
    std::fs::create_dir(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    machine.disarm()?;
    let snapshot = machine.session.checkpoint(None).and_then(|snapshot| snapshot.export());
    machine.rearm()?;
    write(&partial.join(SNAPSHOT), &snapshot?)?;
    write(&partial.join(record_file), format!("{record:#}\n").as_bytes())?;
    let target = root.join(name);
    if target.exists() {
        let stale = root.join(format!(".stale-{name}-{}", std::process::id()));
        std::fs::rename(&target, &stale).map_err(|e| e.to_string())?;
        std::fs::rename(&partial, &target).map_err(|e| e.to_string())?;
        std::fs::remove_dir_all(&stale).map_err(|e| e.to_string())?;
    } else {
        std::fs::rename(&partial, &target).map_err(|e| e.to_string())?;
    }
    Ok(target)
}

pub fn restore(inputs: &Inputs, directory: &Path) -> Result<Machine, String> {
    let mut machine = inputs.machine()?;
    let snapshot = machine.session.import_snapshot(&read(&directory.join(SNAPSHOT))?)?;
    machine.disarm()?;
    machine.session.restore(&snapshot)?;
    machine.rearm()?;
    Ok(machine)
}
