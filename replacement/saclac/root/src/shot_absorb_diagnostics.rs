#![forbid(unsafe_code)]

use xm1_saclac_calc::{adjust::shot_trace::{self, PlainTrace, TraceSlot}, diagnostics::{self, RateStatus}, exposure::ports::Placement, scene_recognition::ports::Overlay, stock::calculation::Pass};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{calc::SharedAddress, stock::record_handle_from_a2, ImageProcessor};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries};
use xm1_saclac_shared::{settings::{AdjustmentKey, Adjustments}, stock::Machine};
use xm1_shot_sequence::{absorb::{AbsorbAdjustment, AbsorbDiagnostics, ResultExport}, boundary::Frame, capture_boundary::AreaReference, stock::calculation};
use crate::{calculation_services::Calculation, shot_services::Libraries, shot_trace_views::{Block, Cells}};

struct RateOverlay<'a, M>(&'a M);
impl<M: Machine> Overlay for RateOverlay<'_, M> {
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8) {
        crate::round_scene::overlay_text(self.0, text, column, row, foreground, background);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RateTenths(pub u16);

pub(crate) fn draw_rate_status(machine: &impl Machine, tenths: RateTenths, status: RateStatus, second_column: bool) {
    diagnostics::draw_rate_status(&mut RateOverlay(machine), u32::from(tenths.0), status, second_column);
}

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor, I: Isp, T: Tables, A: Placement, Q>
    AbsorbDiagnostics for Libraries<'_, M, R, E, P, I, T, A, Q> {
    fn adjustment(&mut self, adjustment: AbsorbAdjustment) -> u8 {
        let key = match adjustment { AbsorbAdjustment::RetainedExport => 0x65a8, AbsorbAdjustment::Subjects => 0x5256, AbsorbAdjustment::RateStatus => 0x5258 };
        self.services.borrow().stock.byte(AdjustmentKey(key))
    }
    fn export_result(&mut self, frame: Option<Frame>, export: ResultExport, enabled: bool) {
        let Some(frame) = frame.filter(|frame| frame.0 != 0) else { return };
        let machine = self.services.borrow().machine.clone();
        let frame = Some(SharedAddress(frame.0));
        let mut block = Block(&machine);
        if export == ResultExport::Compact {
            shot_trace::trace_area_grid(&mut *self.services.borrow_mut(), &mut block, frame, TraceSlot::Repeat);
            return;
        }
        let record = record_handle_from_a2(machine.read_u32(calculation::BLOCK));
        let context = self.exposure.context.borrow();
        let mut ports = Calculation { services: &mut self.services.borrow_mut(), geometry: &mut self.geometry.borrow_mut(), placement: &mut *self.placement.borrow_mut() };
        let slot = if enabled { TraceSlot::Repeat } else { TraceSlot::First };
        match export {
            ResultExport::Policy => shot_trace::trace_plain_selection(&mut ports, &mut block, &mut Cells(&machine), &PlainTrace {
                frame, count: machine.read_u32(calculation::INPUT + 0x14), levels: &context.levels, record,
                lens_reference: context.binding.lens_reference, skip_grids: enabled,
            }),
            ResultExport::SelectedChannel => shot_trace::trace_selected_cell(&mut ports, &mut block, &mut Cells(&machine), frame, slot, Pass::Plain),
            ResultExport::SourceLimits => shot_trace::trace_lens_levels(&mut ports, &mut block, record, frame, slot, context.binding.lens_reference),
            ResultExport::Compact => unreachable!(),
        }
    }
    fn clear_channel_diagnostics(&mut self, frame: Option<Frame>, cleared: bool) {
        let Some(frame) = frame.filter(|frame| frame.0 != 0) else { return };
        diagnostics::clear_selection(&mut *self.services.borrow_mut(), Some(SharedAddress(frame.0)), cleared);
    }
    fn draw_rate_status(&mut self, reference: &AreaReference) {
        draw_rate_status(&self.services.borrow().machine, RateTenths(reference.0[5] as u16), RateStatus::from_code(reference.0[4]), false);
    }
    fn release_rate_status(&mut self) { self.services.borrow().machine.enter(0x111_0a84, &[]); }
}
