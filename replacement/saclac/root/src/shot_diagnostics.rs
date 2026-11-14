#![forbid(unsafe_code)]

use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::boundary::{Diagnostics, Dump, Frame, Trace};
use crate::shot_services::Libraries;

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
        xm1_saclac_search_bindings::stock::reset_focus_log(&mut self.services.borrow().machine.clone());
    }
}
