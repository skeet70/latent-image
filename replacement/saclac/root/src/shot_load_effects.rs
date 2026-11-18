#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::shot_trace::{self, TraceSlot};
use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::Frame, record_load::{AuxiliaryCall, LoadEffects, LoadExport}, stock::record_load as wire};
use crate::{shot_services::Libraries, shot_trace_views::{Block, Cells}};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P, I, T, A, Q> LoadEffects for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn export_loaded(&mut self, frame: Option<Frame>, export: LoadExport) {
        let Some(frame) = frame else { return };
        let machine = self.services.borrow().machine.clone();
        let mut block = Block(&machine);
        let mut cells = Cells(&machine);
        let frame = Some(SharedAddress(frame.0));
        match export {
            LoadExport::Areas => {
                let context = self.exposure.context.borrow();
                shot_trace::trace_selection(&mut *self.services.borrow_mut(), &mut block, &mut cells, frame, u32::from(context.selection.count), &context.levels);
            }
            LoadExport::Cell => {
                let pass = shot_trace::mode_pass(&mut block);
                shot_trace::trace_selected_cell(&mut *self.services.borrow_mut(), &mut block, &mut cells, frame, TraceSlot::First, pass);
            }
        }
    }
    fn call_auxiliary_hook(&mut self, call: &AuxiliaryCall) {
        let arguments = wire::auxiliary_hook_arguments(call).map(|word| word as usize);
        let machine = self.services.borrow().machine.clone();
        machine.enter(call.hook.0, &arguments);
    }
    fn publish_auxiliary_call(&mut self, call: &AuxiliaryCall) {
        let mut machine = self.services.borrow().machine.clone();
        let bytes = wire::auxiliary_publication(call);
        // f1a018..30 copies all48 bytes between the interface lock and unlock.
        machine.enter(0x723efc, &[]);
        for (index, word) in bytes.chunks_exact(4).enumerate() {
            machine.write_u32(wire::AUXILIARY_PUBLICATION.wrapping_add(4 * index as u32), u32::from_le_bytes(word.try_into().expect("auxiliary publication word")));
        }
        machine.enter(0x723f7c, &[]);
    }
}
