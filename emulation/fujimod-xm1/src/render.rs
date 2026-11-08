//! Key presses bounded by the X-M1 UI's render boundary, after the retained
//! `tools/menu_render_settle.py`: drain the preceding packet before pressing,
//! then run until the press's UILIB packet is consumed and the UI drains. The
//! selected-plane publication that the retained settle also requires is not
//! read here.

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Graphics {
    pub idle: bool,
    pub started: u64,
    pub completed: u64,
    pub bytes_completed: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Probe {
    pub copied_command: u16,
    pub packet_token: u8,
    pub sender_token: u8,
    pub consumer_token: u8,
    pub sender_command: u16,
    pub staged_render_command: u16,
    pub staged_render_token: u8,
    pub drained: bool,
    pub uilib_waiting: bool,
    pub graphics: Graphics,
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

/// The gate's inputs at one sample.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GateSample {
    pub keys_released: bool,
    pub ui_pending: u32,
    pub ui_busy: u32,
    pub display_suppression: u32,
    pub batch_busy: u32,
    pub batch_callback: u32,
    pub commands_dirty: u32,
    pub display_pending: u32,
    pub initialized: u32,
    /// Status bytes in `task_entries` order; `None` for an absent task.
    pub tasks: Vec<Option<u8>>,
    pub graphics: GraphicsGate,
    pub probe: Probe,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct GraphicsGate {
    pub busy: bool,
    pub command_pending: bool,
    pub status: u32,
    pub started: u64,
    pub completed: u64,
    pub acknowledgments: u64,
}

/// Per gate condition, the samples of one drain or settle loop in which it was unmet.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct GateTally {
    pub samples: u64,
    pub keys_held: u64,
    pub ui_pending: u64,
    pub ui_busy: u64,
    pub display_suppression: u64,
    pub batch_busy: u64,
    pub batch_callback: u64,
    pub commands_dirty: u64,
    pub display_pending: u64,
    pub uninitialized: u64,
    pub graphics_busy: u64,
    /// In `task_entries` order.
    pub tasks_not_waiting: Vec<u64>,
    pub consumer_unfinished: u64,
}

impl GateTally {
    fn add(&mut self, sample: &GateSample) {
        let count = |held: bool, counter: &mut u64| *counter += u64::from(held);
        self.samples += 1;
        count(!sample.keys_released, &mut self.keys_held);
        count(sample.ui_pending != 0, &mut self.ui_pending);
        count(sample.ui_busy != 0, &mut self.ui_busy);
        count(sample.display_suppression != 0, &mut self.display_suppression);
        count(sample.batch_busy != 0, &mut self.batch_busy);
        count(sample.batch_callback != 0, &mut self.batch_callback);
        count(sample.commands_dirty != 0, &mut self.commands_dirty);
        count(sample.display_pending != 0, &mut self.display_pending);
        count(sample.initialized != 1, &mut self.uninitialized);
        count(!sample.probe.graphics.idle, &mut self.graphics_busy);
        self.tasks_not_waiting.resize(sample.tasks.len(), 0);
        for (counter, status) in self.tasks_not_waiting.iter_mut().zip(&sample.tasks) {
            count(*status != Some(QUEUE_WAIT), counter);
        }
        count(!sample.probe.consumer_finished(), &mut self.consumer_unfinished);
    }
}

/// The gate's inputs over a loop that ran out of budget.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GateReport {
    pub tally: GateTally,
    pub first: GateSample,
    pub last: GateSample,
}

#[derive(Clone, Debug, Serialize)]
pub struct Settle {
    pub status: SettleStatus,
    pub prior_instructions: u64,
    pub key_instructions: u64,
    pub settle_instructions: u64,
    /// Present when the budget ran out.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gate: Option<GateReport>,
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
    pub(crate) fn task_statuses(&self, machine: &Machine) -> Result<Vec<Option<u8>>, String> {
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

    pub fn probe(&self, machine: &Machine) -> Result<Probe, String> {
        Ok(self.sample(machine)?.probe)
    }

    pub fn sample(&self, machine: &Machine) -> Result<GateSample, String> {
        let read = |address: Address, bytes: usize| value(machine, address, bytes);
        let tasks = self.task_statuses(machine)?;
        let keys_released = machine
            .document
            .firmware
            .keys
            .iter()
            .map(|binding| machine.document.firmware.released(&machine.session, binding.key))
            .collect::<Result<Vec<bool>, String>>()?
            .into_iter()
            .all(|released| released);
        let (ui_pending, ui_busy, display_suppression) =
            (read(self.ui_pending, 4)?, read(self.ui_busy, 1)?, read(self.display_suppression, 4)?);
        let (batch_busy, batch_callback, commands_dirty, display_pending) = (
            read(self.batch_busy, 4)?,
            read(self.batch_callback, 4)?,
            read(self.commands_dirty, 4)?,
            read(self.display_pending, 4)?,
        );
        let idle = [ui_pending, ui_busy, display_suppression, batch_busy, batch_callback, commands_dirty, display_pending]
            .iter()
            .all(|word| *word == 0);
        let node = machine.session.node(&self.graphics.0)?;
        let engine = serde_json::from_value::<GraphicsSave>(machine.session.board.device_save(node)?)
            .map_err(|e| format!("graphics state: {e}"))?
            .engine;
        let engine_gate = GraphicsGate {
            busy: engine.busy,
            command_pending: !engine.command_pending.is_null(),
            status: engine.regs.iter().find(|(offset, _)| *offset == 0x34).map_or(0, |(_, value)| *value),
            started: engine.jobs_started,
            completed: engine.jobs_completed,
            acknowledgments: engine.acknowledgments,
        };
        let graphics = Graphics {
            idle: !engine_gate.busy
                && !engine_gate.command_pending
                && engine_gate.status == 0
                && engine.jobs_started == engine.jobs_completed
                && engine.acknowledgments >= engine.jobs_completed,
            started: engine.jobs_started,
            completed: engine.jobs_completed,
            bytes_completed: engine.bytes_completed,
        };
        let initialized = read(self.initialized, 1)?;
        let drained = keys_released
            && idle
            && graphics.idle
            && initialized == 1
            && tasks.iter().all(|status| *status == Some(QUEUE_WAIT));
        let probe = Probe {
            copied_command: read(self.copied_command, 2)? as u16,
            packet_token: read(self.packet_token, 1)? as u8,
            sender_token: read(self.sender_token, 1)? as u8,
            consumer_token: read(self.consumer_token, 1)? as u8,
            sender_command: read(self.sender_command, 2)? as u16,
            staged_render_command: read(self.staged_render_command, 2)? as u16,
            staged_render_token: read(self.staged_render_token, 1)? as u8,
            drained,
            uilib_waiting: tasks.last().copied().flatten() == Some(QUEUE_WAIT),
            graphics,
        };
        Ok(GateSample {
            keys_released,
            ui_pending,
            ui_busy,
            display_suppression,
            batch_busy,
            batch_callback,
            commands_dirty,
            display_pending,
            initialized,
            tasks,
            graphics: engine_gate,
            probe,
        })
    }
}

impl Probe {
    pub fn auxiliary_acknowledged(&self) -> bool {
        self.copied_command == 0x8000
            && self.sender_command == 0x8000
            && self.packet_token == 0
            && self.staged_render_command & 0xf000 == 0x4000
            && self.staged_render_token == self.sender_token
            && self.sender_token == self.consumer_token
            && self.sender_token != 0
    }

    pub fn consumer_finished(&self) -> bool {
        self.uilib_waiting
            && self.sender_token != 0
            && (self.packet_token == self.sender_token && self.sender_token == self.consumer_token
                || self.auxiliary_acknowledged())
            && self.copied_command == self.sender_command
    }
}

/// Presses `key` once the preceding render has drained, then settles; `budget`
/// caps the instructions run for both. An unlatched press fails before the settle.
pub fn settled_key(machine: &mut Machine, key: Press, pulse: KeyPulse, budget: u64) -> Result<Settle, String> {
    let sample = |machine: &Machine| machine.document.firmware.render.sample(machine);
    let start = machine.instructions();
    let mut tally = GateTally::default();
    let first = sample(machine)?;
    tally.add(&first);
    let mut latest: Option<GateSample> = None;
    loop {
        let current = latest.as_ref().unwrap_or(&first);
        if current.probe.drained && current.probe.consumer_finished() {
            break;
        }
        machine.require_running()?;
        let spent = machine.instructions() - start;
        if spent >= budget {
            let last = latest.unwrap_or_else(|| first.clone());
            return Ok(Settle {
                status: SettleStatus::PriorRenderPending,
                prior_instructions: spent,
                key_instructions: 0,
                settle_instructions: 0,
                gate: Some(GateReport { tally, first, last }),
            });
        }
        machine.run((budget - spent).min(CHUNK));
        let next = sample(machine)?;
        tally.add(&next);
        latest = Some(next);
    }
    let before = latest.as_ref().unwrap_or(&first).probe;
    let prior = machine.instructions() - start;
    let old_frame = machine.frame()?;
    let key_start = machine.instructions();
    machine.press_latched(key, pulse)?;
    let settle_start = machine.instructions();
    let mut requested = None;
    let mut tally = GateTally::default();
    let mut first: Option<GateSample> = None;
    let mut latest: Option<GateSample> = None;
    let status = loop {
        machine.require_running()?;
        let sampled = sample(machine)?;
        tally.add(&sampled);
        let after = sampled.probe;
        if first.is_none() {
            first = Some(sampled);
        } else {
            latest = Some(sampled);
        }
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
        gate: match (status, first) {
            (SettleStatus::BudgetExhausted, Some(first)) => {
                let last = latest.unwrap_or_else(|| first.clone());
                Some(GateReport { tally, first, last })
            }
            _ => None,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GateSample {
        GateSample {
            keys_released: true,
            ui_pending: 0,
            ui_busy: 0,
            display_suppression: 0,
            batch_busy: 0,
            batch_callback: 0,
            commands_dirty: 0,
            display_pending: 0,
            initialized: 1,
            tasks: vec![Some(QUEUE_WAIT); 4],
            graphics: GraphicsGate { busy: false, command_pending: false, status: 0, started: 3, completed: 3, acknowledgments: 3 },
            probe: Probe {
                copied_command: 0x4001,
                packet_token: 1,
                sender_token: 1,
                consumer_token: 1,
                sender_command: 0x4001,
                staged_render_command: 0,
                staged_render_token: 0,
                drained: true,
                uilib_waiting: true,
                graphics: Graphics { idle: true, started: 3, completed: 3, bytes_completed: 30 },
            },
        }
    }

    #[test]
    fn tally_counts_each_condition_that_held_the_gate() {
        let mut tally = GateTally::default();
        tally.add(&sample());
        let mut held = sample();
        held.display_pending = 1;
        held.tasks[1] = Some(0x02);
        held.probe.graphics.idle = false;
        held.probe.consumer_token = 0;
        tally.add(&held);
        tally.add(&held);
        assert_eq!(
            tally,
            GateTally {
                samples: 3,
                display_pending: 2,
                graphics_busy: 2,
                tasks_not_waiting: vec![0, 2, 0, 0],
                consumer_unfinished: 2,
                ..GateTally::default()
            }
        );
    }

    #[test]
    fn settle_omits_the_gate_when_absent() {
        let settle = Settle {
            status: SettleStatus::PacketConsumed,
            prior_instructions: 1,
            key_instructions: 2,
            settle_instructions: 3,
            gate: None,
        };
        let value = serde_json::to_value(&settle).expect("settle serializes");
        assert_eq!(value, serde_json::json!({"status": "packet-consumed", "prior_instructions": 1, "key_instructions": 2, "settle_instructions": 3}));
    }
}
