//! Labelled instruction points on every core and the entries recorded at them.

use crate::provenance::Guest;
use fujimod_emulator::board::observation::InstructionPoints;
use fujimod_emulator::session::Session;
use fujimod_emulator_cpu::{Register, arm};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// One entry into an armed site, before its instruction executes.
#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub site: String,
    pub address: String,
    pub core: usize,
    pub instructions: u64,
    pub registers: BTreeMap<&'static str, String>,
    pub sp: String,
    pub lr: String,
    pub cpsr: u32,
}

#[derive(Clone, Copy, Default)]
enum CoreSelection {
    #[default]
    All,
    Primary,
}

struct EnteredInstructions(u64);
struct ResumeMarker {
    pc: Guest,
    entered: EnteredInstructions,
}

#[derive(Default)]
pub struct TraceSites {
    pub sites: BTreeMap<u32, String>,
    /// Labels whose entry ends the current run.
    pub stop_labels: BTreeSet<String>,
    pub hits: Vec<Hit>,
    points: Vec<(usize, Box<InstructionPoints>)>,
    cores: CoreSelection,
    resumed: BTreeMap<usize, ResumeMarker>,
}

fn register(session: &Session, core: usize, register: arm::Register) -> Result<u32, String> {
    let value = session.board.read_register(core, Register::Arm32(register))?;
    u32::try_from(value).map_err(|_| "ARM register exceeds 32 bits".to_owned())
}

impl TraceSites {
    pub fn armed(&self) -> bool {
        !self.points.is_empty()
    }

    /// Replaces the armed sites on every core.
    pub fn arm(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::All)
    }

    pub fn arm_primary(&mut self, session: &Session, sites: BTreeMap<u32, String>) -> Result<(), String> {
        self.arm_selection(session, sites, CoreSelection::Primary)
    }

    fn arm_selection(
        &mut self,
        session: &Session,
        sites: BTreeMap<u32, String>,
        cores: CoreSelection,
    ) -> Result<(), String> {
        self.disarm(session)?;
        self.sites = sites;
        self.cores = cores;
        self.resumed.clear();
        self.rearm(session)
    }

    pub fn disarm(&mut self, session: &Session) -> Result<(), String> {
        for (core, mut points) in self.points.drain(..) {
            session.board.instruction_points_end(core, &mut points)?;
        }
        Ok(())
    }

    pub fn remove_site(&mut self, session: &Session, address: Guest) -> Result<(), String> {
        if !self.sites.contains_key(&address.0) {
            return Err("trace site is not armed".into());
        }
        self.disarm(session)?;
        let label = self.sites.remove(&address.0).ok_or("trace site disappeared")?;
        if !self.sites.values().any(|other| other == &label) {
            self.stop_labels.remove(&label);
        }
        self.rearm(session)?;
        let counts = session.board.instruction_counts();
        for (core, points) in &self.points {
            if let Some(marker) = self.resumed.get(core) {
                let current = session.board.read_register(*core, Register::Pc)?;
                if current == u64::from(marker.pc.0) && counts[*core] == marker.entered.0 {
                    points.resume(current, u64::from(marker.pc.0));
                }
            }
        }
        Ok(())
    }

    pub fn rearm(&mut self, session: &Session) -> Result<(), String> {
        if !self.points.is_empty() {
            return Err("trace sites are already armed".into());
        }
        if self.sites.is_empty() {
            return Ok(());
        }
        let addresses: Vec<u64> = self.sites.keys().map(|&address| u64::from(address)).collect();
        for core in 0..session.board.instruction_counts().len() {
            let selected = match self.cores {
                CoreSelection::All => true,
                CoreSelection::Primary => core == 0,
            };
            if !selected {
                continue;
            }
            let points = session.board.instruction_points_begin(core, addresses.clone())?;
            self.points.push((core, points));
        }
        Ok(())
    }

    /// Records pending entries and lets their instructions run on resume.
    /// Returns whether one of them is a stop label.
    pub fn collect(&mut self, session: &Session) -> Result<bool, String> {
        let mut stop = false;
        for (core, points) in &self.points {
            let Some(pc) = points.event() else { continue };
            let address = u32::try_from(pc).map_err(|_| "trace site exceeds 32 bits")?;
            let site = self.sites.get(&address).cloned().ok_or("instruction point outside the armed sites")?;
            let registers = (0..4u8)
                .map(|index| {
                    let name = ["r0", "r1", "r2", "r3"][usize::from(index)];
                    register(session, *core, arm::Register::R(index)).map(|value| (name, format!("{value:#x}")))
                })
                .collect::<Result<_, String>>()?;
            let counts = session.board.instruction_counts();
            let instructions = counts.iter().sum();
            stop |= self.stop_labels.contains(&site);
            self.hits.push(Hit {
                site,
                address: format!("{address:#x}"),
                core: *core,
                instructions,
                registers,
                sp: format!("{:#x}", register(session, *core, arm::Register::R(13))?),
                lr: format!("{:#x}", register(session, *core, arm::Register::R(14))?),
                cpsr: register(session, *core, arm::Register::Cpsr)?,
            });
            let current = session.board.read_register(*core, Register::Pc)?;
            points.resume(current, pc);
            if current == pc {
                self.resumed.insert(*core, ResumeMarker { pc: Guest(address), entered: EnteredInstructions(counts[*core]) });
            } else {
                self.resumed.remove(core);
            }
        }
        Ok(stop)
    }
}
