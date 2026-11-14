#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

//! Record-bound search ports over the composition root's borrowed services.

pub mod calc;
mod ports;
pub mod stock;

use core::cell::{Cell, RefCell};
use xm1_focus_measurement_round::history::MeasurementHistory;
use xm1_saclac_calc::adjust::{
    configuration::Configuration,
    scan_drive::{SampleTiming, ScanDrive},
};
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::{Memory, ProfileRecords, StockEntries};
use xm1_saclac_shared::stock::Machine;

/// One instance belongs in Services and is lent to both acquisition and search.
#[derive(Default)]
pub struct State {
    pub configuration: Configuration,
    pub scan_drive: ScanDrive,
    history: Option<[Option<[u16; 2]>; 5]>,
}

impl State {
    pub fn set_sample_timing(&mut self, timing: SampleTiming) {
        self.scan_drive.set_timing(timing);
    }
}

pub struct Bindings<'a, M, R, E, P> {
    pub machine: &'a mut M,
    pub records: &'a mut ProfileRecords<R>,
    pub entries: &'a mut E,
    pub peers: &'a mut P,
    pub state: &'a mut State,
}

impl<'a, M: Machine, R: Memory, E: StockEntries, P> Bindings<'a, M, R, E, P> {
    pub fn new(
        machine: &'a mut M,
        records: &'a mut ProfileRecords<R>,
        entries: &'a mut E,
        peers: &'a mut P,
        state: &'a mut State,
    ) -> Self {
        Self {
            machine,
            records,
            entries,
            peers,
            state,
        }
    }

    /// Refresh before dispatching a measurement event; the round owns the history itself.
    pub fn set_history(&mut self, history: &MeasurementHistory, kept: bool) {
        self.state.history = (kept && history.started()).then(|| {
            let mut entries = [None; 5];
            for (out, entry) in entries.iter_mut().zip(history.recent()) {
                *out = entry.written().then(|| entry.cell_levels());
            }
            entries
        });
    }

    pub fn calc_ports(&mut self) -> calc::Ports<'_, M, R, E> {
        calc::Ports {
            machine: self.machine,
            records: self.records,
            entries: self.entries,
        }
    }
}

/// Updated from the lane's record or ChildEvent before each dispatch.
pub struct RecordBinding(Cell<RecordHandle>);
impl RecordBinding {
    pub const fn new(record: RecordHandle) -> Self {
        Self(Cell::new(record))
    }
    pub fn set(&self, record: RecordHandle) {
        self.0.set(record);
    }
}

/// Copies share services, planner state and the record selector, not their contents.
pub struct Session<'s, 'a, M, R, E, P> {
    bindings: &'s RefCell<Bindings<'a, M, R, E, P>>,
    record: &'s RecordBinding,
}
impl<M, R, E, P> Copy for Session<'_, '_, M, R, E, P> {}
impl<M, R, E, P> Clone for Session<'_, '_, M, R, E, P> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<'s, 'a, M, R, E, P> Session<'s, 'a, M, R, E, P> {
    pub const fn new(
        bindings: &'s RefCell<Bindings<'a, M, R, E, P>>,
        record: &'s RecordBinding,
    ) -> Self {
        Self { bindings, record }
    }
}

#[cfg(test)]
mod tests;
