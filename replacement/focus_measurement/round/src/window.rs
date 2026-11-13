#![forbid(unsafe_code)]

//! The window calculation's peer work: its request to calc, its result in the
//! shared record and the measurement power it holds.

use crate::jobs;
use crate::pipeline::{PipelineSnapshot, Statistics};
use crate::profile::{ProfileMode, ProfileSelection, StatisticsInUse, WindowPosition, WindowRecord, WindowStatus};
use crate::record::{Ports as RecordPorts, Request, TraceMark};
use xm1_saclac_interface::{Completed, RecordHandle};

/// What calc receives for one window calculation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowRequest {
    pub selection: ProfileSelection,
    pub mode: ProfileMode,
    /// The profile selects by record, or has a measurement mode.
    pub selected: bool,
    pub snapshot: PipelineSnapshot,
    /// The snapshot's first level, offset by the second and by the exposure value.
    pub level: u16,
    /// The snapshot's second level less the gain offset.
    pub reference: u16,
    pub record: WindowRecord,
    pub refresh: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WindowResult {
    pub status: WindowStatus,
    pub position: WindowPosition,
}

pub trait Ports: RecordPorts {
    /// `None` when the completion carries no result.
    fn window_result(&mut self, completed: Completed<'_>) -> Option<WindowResult>;
}

pub struct Window {
    power_held: bool,
}

impl Default for Window { fn default() -> Self { Self::new() } }

impl Window {
    pub const fn new() -> Self { Self { power_held: false } }

    /// `completed` is the completion a `Collect` follows.
    pub fn effect(&mut self, effect: jobs::Effect, record: RecordHandle, completed: Option<Completed<'_>>, ports: &mut impl Ports) {
        match effect {
            jobs::Effect::Initialize => self.power_held = false,
            jobs::Effect::Enable => {
                let in_use = ports.statistics_in_use();
                ports.set_statistics_in_use(record, StatisticsInUse { primary: true, ..in_use });
            }
            jobs::Effect::Release => self.release_power(ports),
            jobs::Effect::Submit => self.submit(record, ports),
            jobs::Effect::Collect => {
                if let Some(result) = completed.and_then(|completed| ports.window_result(completed)) {
                    self.collect(record, result, ports);
                }
            }
        }
    }

    fn submit(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let profile = ports.profile(record);
        let snapshot = ports.snapshot(Statistics::Primary);
        let reference = snapshot.levels[1].wrapping_sub(ports.gain_offset(snapshot.gain_step) as u16);
        let exposure = ports.exposure_value(snapshot.exposure) as u16;
        let level = snapshot.levels[0].wrapping_sub(reference).wrapping_add(exposure);
        let level = if snapshot.adjustment.0 == 1 { level.wrapping_sub(100) } else { level };
        let window = ports.window();
        let refresh = ports.take_window_refresh();
        if !self.power_held && window.status.holds_power() {
            ports.request_measurement_power(true);
            self.power_held = true;
        }
        if ports.traces_measurement() { ports.trace(TraceMark::WindowRequest); }
        ports.request(Request::Window(WindowRequest {
            selection: profile.selection,
            mode: profile.mode,
            selected: profile.selection == ProfileSelection::Record || profile.mode.set(),
            snapshot,
            level,
            reference,
            record: window,
            refresh,
        }));
    }

    fn collect(&mut self, record: RecordHandle, result: WindowResult, ports: &mut impl Ports) {
        ports.store_window_status(record, result.status);
        if !result.status.holds_power() { self.release_power(ports); }
        ports.store_window_position(record, result.position);
    }

    fn release_power(&mut self, ports: &mut impl Ports) {
        if self.power_held {
            ports.request_measurement_power(false);
            self.power_held = false;
        }
    }
}
