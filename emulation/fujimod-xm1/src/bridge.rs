//! Pieces the JSON-lines bridges share: their inputs, JSON output and
//! session checkpoints saved as FujiMod snapshots.

use crate::adjustment::AdjustmentNand;
use crate::guest::MenuState;
use crate::machine::Machine;
use crate::record_medium::RecordMedium;
use crate::nand_image::CommittedImage;
use crate::run::{self, RunDocument, ValidatedUpdate};
use crate::sd_host::{self, Backing, RetainedCard};
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
    pub nand: BootSource,
    /// The card inserted before the first instruction; absent leaves the slot empty.
    pub card: Option<RetainedCard>,
}

pub enum BootSource {
    InitialSeeded { adjustment: AdjustmentNand, records: Option<RecordMedium> },
    FullCommitted(CommittedImage),
}


impl Inputs {
    pub fn into_machine(self) -> Result<(Machine, CommittedImage), String> {
        let document = RunDocument::parse(&self.document_bytes)?;
        let update = ValidatedUpdate(self.update);
        let mut session = run::open(&document, &update, &self.boot_rom)?;
        session.board.set_execution_policy(document.execution_policy.into())?;
        let template = match self.nand {
            BootSource::InitialSeeded { adjustment, records } => {
                adjustment.apply(&mut session, &document.nand)?;
                if let Some(records) = records {
                    records.apply(&mut session, &document.nand)?;
                }
                CommittedImage::from_medium(&session.nand_medium(&document.nand.0)?)?
            }
            BootSource::FullCommitted(mut image) => {
                image.apply_owned(&mut session, &document.nand, run::nand_base(&document, &update)?.as_ref())?;
                image
            }
        };
        let mut machine = Machine::new(document, session)?;
        if let Some(card) = self.card {
            machine.insert_card(card)?;
        }
        Ok((machine, template))
    }

    pub fn machine(&self) -> Result<Machine, String> {
        let document = RunDocument::parse(&self.document_bytes)?;
        let update = ValidatedUpdate(self.update.clone());
        let mut session = run::open(&document, &update, &self.boot_rom)?;
        session.board.set_execution_policy(document.execution_policy.into())?;
        match &self.nand {
            BootSource::InitialSeeded { adjustment, records } => {
                adjustment.apply(&mut session, &document.nand)?;
                if let Some(records) = records {
                    records.apply(&mut session, &document.nand)?;
                }
            }
            BootSource::FullCommitted(image) => image.apply(&mut session, &document.nand, run::nand_base(&document, &update)?.as_ref())?,
        }
        let mut machine = Machine::new(document, session)?;
        if let Some(card) = &self.card {
            machine.insert_card(card.clone())?;
        }
        Ok(machine)
    }
}

/// Loads the raw image at `path` as a card of the document's SD host.
pub fn load_card(document_bytes: &[u8], path: &Path, backing: Backing) -> Result<RetainedCard, String> {
    let document = RunDocument::parse(document_bytes)?;
    let (sectors, max_dirty_sectors) = sd_host::card_geometry(&document.sd_host()?)?;
    sd_host::load_image(path, sectors, max_dirty_sectors, backing)
}
#[derive(Clone,Copy)]
pub struct CheckpointBudget(std::num::NonZeroU64);
impl CheckpointBudget {
    pub fn new(bytes:u64)->Result<Self,String> {
        Ok(Self(std::num::NonZeroU64::new(bytes).ok_or("checkpoint budget must be positive")?))
    }
    pub(crate) fn admit(self,snapshot:u64,record:u64)->Result<(),String> {
        if snapshot.checked_add(record).is_none_or(|bytes|bytes>self.0.get()) {
            return Err("checkpoint retained-byte budget exhausted".into());
        }
        Ok(())
    }
}


/// Publishes through a partial directory. Unbounded saves replace an existing
/// checkpoint; bounded saves refuse an existing checkpoint.
pub fn save_checkpoint(machine: &mut Machine, root: &Path, name: &str, record_file: &str, record: &Value, budget:Option<CheckpointBudget>) -> Result<PathBuf, String> {
    std::fs::create_dir_all(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let partial = root.join(format!(".partial-{name}-{}", std::process::id()));
    std::fs::create_dir(&partial).map_err(|e| format!("{}: {e}", partial.display()))?;
    machine.disarm()?;
    let snapshot = machine.session.checkpoint(None).and_then(|snapshot| snapshot.export());
    machine.rearm()?;
    let snapshot=snapshot?;
    let record=format!("{record:#}\n");
    if let Some(budget)=budget {budget.admit(snapshot.len() as u64,record.len() as u64)?;}
    let target=root.join(name);
    if budget.is_some() && target.exists() {return Err("bounded checkpoint already exists".into());}
    write(&partial.join(SNAPSHOT), &snapshot)?;
    write(&partial.join(record_file), record.as_bytes())?;
    if target.exists() {
        if budget.is_some() {return Err("bounded checkpoint already exists".into());}
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
