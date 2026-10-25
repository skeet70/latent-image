//! Key presses bounded by the X-M1 UI's render boundary, after the retained
//! `tools/menu_render_settle.py`: drain the preceding packet before pressing,
//! then run until the press's UILIB packet is consumed and the UI drains. The
//! graphics-accelerator counters and selected-plane publication that the
//! retained settle also requires are not read here.

use crate::guest::Press;
use crate::machine::{KeyPulse, Machine};
use crate::run::{Address, NodeName};
use serde::{Deserialize, Serialize};

/// The default cap on a key's prior drain plus settle.
pub const MAX_SETTLE: u64 = 90_000_000;
const CHUNK: u64 = 250_000;
const QUEUE_WAIT: u8 = 0x12;
const ALIAS: u64 = 0x4000_0000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderFacts {
    pub copied_command: Address,
    pub packet_token: Address,
    pub initialized: Address,
    pub display_suppression: Address,
    pub sender_token: Address,
    pub consumer_token: Address,
    pub sender_command: Address,
    pub staged_render_command: Address,
    pub staged_render_token: Address,
    pub batch_busy: Address,
    pub batch_callback: Address,
    pub commands_dirty: Address,
    pub display_pending: Address,
    pub ui_pending: Address,
    pub ui_busy: Address,
    /// `(core, table record)` pairs; each record holds a task table pointer and its count.
    pub task_tables: Vec<(usize, Address)>,
    /// Entry addresses of the main UI, graphic, VGLIB and UILIB tasks; UILIB last.
    pub task_entries: Vec<Address>,
    /// The vector GPU node whose job counters bound a render.
    pub graphics: NodeName,
}

#[derive(Deserialize)]
struct GraphicsEngine {
    regs: Vec<(u32, u32)>,
    busy: bool,
    command_pending: serde_json::Value,
    jobs_started: u64,
    jobs_completed: u64,
    bytes_completed: u64,
    acknowledgments: u64,
}

#[derive(Deserialize)]
struct GraphicsSave {
    engine: GraphicsEngine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Graphics {
    idle: bool,
    started: u64,
    completed: u64,
    bytes_completed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
struct Probe {
    copied_command: u16,
    packet_token: u8,
    sender_token: u8,
    consumer_token: u8,
    sender_command: u16,
    staged_render_command: u16,
    staged_render_token: u8,
    drained: bool,
    uilib_waiting: bool,
    graphics: Graphics,
}

/// Where a settled press ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SettleStatus {
    /// The preceding render never drained, so the key was not pressed.
    PriorRenderPending,
    /// The press's packet was consumed and the UI drained.
    PacketConsumed,
    BudgetExhausted,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Settle {
    pub status: SettleStatus,
    pub prior_instructions: u64,
    pub key_instructions: u64,
    pub settle_instructions: u64,
}

fn physical(address: Address) -> u64 {
    if (ALIAS..ALIAS + 0x2000_0000).contains(&address.0) { address.0 - ALIAS } else { address.0 }
}

fn value(machine: &Machine, address: Address, bytes: usize) -> Result<u32, String> {
    let raw = machine.read(physical(address), bytes)?;
    Ok(raw.iter().rev().fold(0, |acc, byte| acc << 8 | u32::from(*byte)))
}

impl RenderFacts {
    /// Task statuses by entry, in `task_entries` order; absent tasks are None.
    fn task_statuses(&self, machine: &Machine) -> Result<Vec<Option<u8>>, String> {
        let mut statuses = vec![None; self.task_entries.len()];
        for (_, record) in &self.task_tables {
            let table = value(machine, *record, 4)?;
            let count = value(machine, Address(record.0 + 4), 4)?;
            if !(1..=128).contains(&count) {
                continue;
            }
            let rows = machine.read(physical(Address(u64::from(table))), count as usize * 0x38)?;
            for row in rows.chunks_exact(0x38) {
                let word = |index: usize| u32::from_le_bytes([row[4 * index], row[4 * index + 1], row[4 * index + 2], row[4 * index + 3]]);
                let descriptor = machine.read(physical(Address(u64::from(word(2)))), 0x28);
                let Ok(descriptor) = descriptor else { continue };
                let entry = u32::from_le_bytes([descriptor[8], descriptor[9], descriptor[10], descriptor[11]]);
                let active = descriptor[36..40].iter().any(|byte| *byte != 0);
                if let Some(index) = self.task_entries.iter().position(|task| active && task.0 == u64::from(entry)) {
                    statuses[index] = Some((word(3) & 0xff) as u8);
                }
            }
        }
        Ok(statuses)
    }

    fn probe(&self, machine: &Machine) -> Result<Probe, String> {
        let read = |address: Address, bytes: usize| value(machine, address, bytes);
        let statuses = self.task_statuses(machine)?;
        let keys_released = machine
            .document
            .firmware
            .keys
            .iter()
            .map(|binding| machine.document.firmware.released(&machine.session, binding.key))
            .collect::<Result<Vec<bool>, String>>()?
            .into_iter()
            .all(|released| released);
        let idle = [
            read(self.ui_pending, 4)?,
            read(self.ui_busy, 1)?,
            read(self.display_suppression, 4)?,
            read(self.batch_busy, 4)?,
            read(self.batch_callback, 4)?,
            read(self.commands_dirty, 4)?,
            read(self.display_pending, 4)?,
        ]
        .iter()
        .all(|word| *word == 0);
        let node = machine.session.node(&self.graphics.0)?;
        let engine = serde_json::from_value::<GraphicsSave>(machine.session.board.device_save(node)?)
            .map_err(|e| format!("graphics state: {e}"))?
            .engine;
        let graphics = Graphics {
            idle: !engine.busy
                && engine.command_pending.is_null()
                && engine.regs.iter().find(|(offset, _)| *offset == 0x34).map_or(0, |(_, value)| *value) == 0
                && engine.jobs_started == engine.jobs_completed
                && engine.acknowledgments >= engine.jobs_completed,
            started: engine.jobs_started,
            completed: engine.jobs_completed,
            bytes_completed: engine.bytes_completed,
        };
        let drained = keys_released
            && idle
            && graphics.idle
            && read(self.initialized, 1)? == 1
            && statuses.iter().all(|status| *status == Some(QUEUE_WAIT));
        Ok(Probe {
            copied_command: read(self.copied_command, 2)? as u16,
            packet_token: read(self.packet_token, 1)? as u8,
            sender_token: read(self.sender_token, 1)? as u8,
            consumer_token: read(self.consumer_token, 1)? as u8,
            sender_command: read(self.sender_command, 2)? as u16,
            staged_render_command: read(self.staged_render_command, 2)? as u16,
            staged_render_token: read(self.staged_render_token, 1)? as u8,
            drained,
            uilib_waiting: statuses.last().copied().flatten() == Some(QUEUE_WAIT),
            graphics,
        })
    }
}

impl Probe {
    fn auxiliary_acknowledged(&self) -> bool {
        self.copied_command == 0x8000
            && self.sender_command == 0x8000
            && self.packet_token == 0
            && self.staged_render_command & 0xf000 == 0x4000
            && self.staged_render_token == self.sender_token
            && self.sender_token == self.consumer_token
            && self.sender_token != 0
    }

    fn consumer_finished(&self) -> bool {
        self.uilib_waiting
            && self.sender_token != 0
            && (self.packet_token == self.sender_token && self.sender_token == self.consumer_token
                || self.auxiliary_acknowledged())
            && self.copied_command == self.sender_command
    }
}

/// Presses `key` once the preceding render has drained, then settles; `budget`
/// caps the instructions run for both.
pub fn settled_key(machine: &mut Machine, key: Press, pulse: KeyPulse, budget: u64) -> Result<Settle, String> {
    let probe = |machine: &Machine| machine.document.firmware.render.probe(machine);
    let start = machine.instructions();
    let mut before = probe(machine)?;
    while !(before.drained && before.consumer_finished()) {
        machine.require_running()?;
        let spent = machine.instructions() - start;
        if spent >= budget {
            return Ok(Settle { status: SettleStatus::PriorRenderPending, prior_instructions: spent, key_instructions: 0, settle_instructions: 0 });
        }
        machine.run((budget - spent).min(CHUNK));
        before = probe(machine)?;
    }
    let prior = machine.instructions() - start;
    let old_frame = machine.frame()?;
    let key_start = machine.instructions();
    machine.press_once(key, pulse)?;
    let settle_start = machine.instructions();
    let mut requested = None;
    let status = loop {
        machine.require_running()?;
        let after = probe(machine)?;
        if after.sender_token != before.sender_token {
            requested = Some(after.sender_token);
        }
        let consumed = requested.is_some_and(|token| {
            after.copied_command & 0xf000 == 0x4000
                && after.packet_token == token
                && after.consumer_token == token
                && after.copied_command == after.sender_command
                || after.auxiliary_acknowledged()
                    && after.staged_render_token == token
                    && before.staged_render_token != token
        });
        let new_work = after.graphics.started > before.graphics.started
            && after.graphics.completed > before.graphics.completed
            && after.graphics.bytes_completed > before.graphics.bytes_completed;
        if consumed && new_work && after.drained && after.consumer_finished() && machine.frame()? != old_frame {
            break SettleStatus::PacketConsumed;
        }
        let spent = prior + machine.instructions() - settle_start;
        if spent >= budget {
            break SettleStatus::BudgetExhausted;
        }
        machine.run((budget - spent).min(CHUNK));
    };
    Ok(Settle {
        status,
        prior_instructions: prior,
        key_instructions: settle_start - key_start,
        settle_instructions: machine.instructions() - settle_start,
    })
}
