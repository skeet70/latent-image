#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::shot_trace, diagnostics};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::{CalcPayload, SharedAddress}, calculation::ReferenceRequest, Calc, Calc1, Completion, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::stock::Machine;
use xm1_shot_sequence::{boundary::{Frame, LensPoint, LensReference}, capture_boundary::{CalcLibrary, CalculationResult, LensSample}, finish_work::FinishLibrary, stock::{calculation, state}};
use crate::{shot_services::Libraries, shot_trace_views::{Block, Cells}};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables, A, Q: Calc + Calc1>
    CalcLibrary for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn request_reference(&mut self, request: &ReferenceRequest, result: CalculationResult, completion: Completion) {
        assert_eq!(completion, Completion::Calculation90);
        let source = match result { CalculationResult::LensReturn => SharedAddress(state::RETURN_RESULT.get()) };
        let block = self.capture.borrow_mut().stage_reference(request).expect("persistent reference input must fit the guest address space");
        let mut machine = self.services.borrow().machine.clone();
        let mut peers = crate::acquisition::CalculationPeers { machine: &mut machine, peers: &mut self.peers };
        xm1_saclac_calc::requests::Peers::calc(&mut peers, &CalcPayload::Calculation {
            block, source, callback: self.callbacks.calculation,
        }).expect("lens-return calculation enqueue");
    }
    fn selection_rate(&mut self, record: RecordHandle, sample: LensSample, control: u32) -> u32 {
        // f2a570..594 supplies a null diagnostic frame after these six scalar arguments.
        FinishLibrary::target_selection(self, record, LensReference(sample.reference), sample.position as i16, sample.state, sample.kind, control, 0)
    }
    fn selection_direction(&mut self, sample: LensSample) -> u32 {
        let point = LensPoint { position: sample.position as i16, detail: 0, reference: LensReference(sample.reference), state: sample.state, kind: sample.kind };
        FinishLibrary::point_selection(self, &point, LensReference(sample.reference), sample.state)
    }
    fn export_selection(&mut self, frame: Frame) {
        if frame.0 == 0 { return; }
        let machine = self.services.borrow().machine.clone();
        let context = self.exposure.context.borrow();
        // f1dc74/f1dce4 points to the published input's count word at16b4f24.
        let count = machine.read_u32(calculation::INPUT + 0x14);
        shot_trace::trace_second_selection(
            &mut *self.services.borrow_mut(), &mut Block(&machine), &mut Cells(&machine),
            Some(SharedAddress(frame.0)), count, &context.levels,
        );
    }
    fn export_joint_tables(&mut self, frame: Frame) {
        diagnostics::export_joint_tables(&mut *self.services.borrow_mut(), SharedAddress(frame.0));
    }
}
