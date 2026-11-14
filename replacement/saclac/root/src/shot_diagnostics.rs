#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::boundary::{Diagnostics, Dump, Frame, Trace};
use crate::shot_services::Libraries;

const HISTORY: u32 = 0x5ffd_3584;
const HISTORY_CURSOR: u32 = 0x016c_2228;
const RECORD_BYTES: u32 = 0x6c;
const RECORDS: u32 = 301;
const TAIL_BYTES: u32 = 0xbc;

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> Diagnostics for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn trace(&mut self, trace: Trace) {
        let machine = self.services.borrow().machine.clone();
        machine.enter(0x72872c, &[xm1_shot_sequence::stock::trace_word(trace) as usize]);
    }
    fn dump(&mut self, dump: Dump, frame: Frame, _argument: u32) {
        let entry = match dump { Dump::Frame => 0xf48f1c, Dump::Lens => 0xf48e74, Dump::Numbered => 0xf48f54 };
        let machine = self.services.borrow().machine.clone();
        // f48f54 captures r0 only; its nonmatching C's extra arguments are not native inputs.
        machine.enter(entry, &[frame.0 as usize]);
    }
    fn calculation_context_mode(&mut self) -> u32 {
        let selection = self.services.borrow_mut().entries.measurement_selection(self.live_search_record.get());
        u32::from(xm1_saclac_calc::profile::selection_code(selection))
    }
    fn dump_calculation(&mut self) {
        reset_history(&mut self.services.borrow().machine.clone());
    }
}

fn enabled(machine: &impl Machine) -> bool { machine.enter(0x69f8fc, &[0x24c]) & 2 != 0 }

/// f47468/f473a4 reset the existing diagnostic buffer; they do not export it or create a recorder.
fn reset_history(machine: &mut impl Machine) {
    if !enabled(machine) { return; }
    machine.write_u32(HISTORY_CURSOR, 0);
    for index in 0..RECORDS {
        // The native per-record initializer rechecks the adjustment independently.
        if !enabled(machine) { continue; }
        let base = HISTORY + index * RECORD_BYTES;
        for (word, value) in [0x7fff_0000, 0x7fff_7fff, 0x0000_7fff, 0x7fff_7fff].into_iter().enumerate() {
            machine.write_u32(base + word as u32 * 4, value);
        }
        for offset in (16..RECORD_BYTES).step_by(4) { machine.write_u32(base + offset, 0); }
    }
    let tail = HISTORY + RECORDS * RECORD_BYTES;
    for offset in (0..TAIL_BYTES).step_by(4) { machine.write_u32(tail + offset, 0); }
}
