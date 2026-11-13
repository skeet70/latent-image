//! Whether the focus result the snapshot took can stand for the current record, and its trace
//! in the client's dump frame.

use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::exposure::context::ExposureContext;
use crate::exposure::ports::{ProfileAccess, Properties, StockMemory};
use crate::policy::SelectionKind;
use crate::stock::{dump, property, record};

/// Traces the snapshot's result into the bound frame, then says whether it can be reused.
pub fn reuse_focus_result(ports: &mut (impl ProfileAccess + Properties + StockMemory), context: &ExposureContext) -> bool {
    let focus = &context.snapshot.focus;
    let frame = context.binding.frame;
    if let Some(frame) = frame {
        let result = [focus.adjustment[0], focus.adjustment[1], focus.adjustment[2], u32::from(focus.adjustment_kind), focus.subject_count];
        for (index, word) in result.into_iter().enumerate() {
            ports.write_u32(dump::reuse_result(frame, index), word);
        }
        ports.write_u32(dump::reuse_out_of_tolerance(frame), 0);
        ports.write_u32(dump::reuse_level(frame), (i32::from(focus.adjustment_level) + 500) as u32);
        ports.write_u32(dump::reuse_limit(frame), (i32::from(focus.limit) + 500) as u32);
        let tolerance = ports.property(property::REUSE_TOLERANCE);
        ports.write_u32(dump::reuse_tolerance(frame), tolerance);
    }
    let binding = &context.binding;
    if binding.mask & 2 == 0 && binding.mode == 1 {
        return true;
    }
    if focus.adjustment[0] == 0 || !context.outcome.adjusting || context.adjustment.state != 0 {
        return false;
    }
    let distance = (i32::from(focus.limit) - i32::from(focus.adjustment_level)).unsigned_abs() as u16;
    if u32::from(distance) >= ports.property(property::REUSE_TOLERANCE) * 10 {
        if let Some(frame) = frame {
            ports.write_u32(dump::reuse_out_of_tolerance(frame), 1);
        }
        return false;
    }
    let current = binding.record.unwrap_or(record::UNBOUND);
    let kind = SelectionKind(u32::from(ports.profile().selection_style(current, false)));
    let selected = ports.profile().measurement_selection(current) == MeasurementSelection::Record;
    if kind.qualifies(selected, context.levels.unmeasured, binding.option != 0) {
        return false;
    }
    if kind.0 != 6 && binding.option != focus.adjustment[1] {
        return false;
    }
    let compares_subjects = ports.profile().read(current, &group_00::AT_005D) != 0;
    if compares_subjects && binding.subjects.count != focus.subject_count {
        return false;
    }
    if ports.profile().header(current, SharedHeaderByte::At63) == 0 {
        return true;
    }
    ports.profile().selection_kind(current, false) == focus.adjustment[2] && focus.adjustment_kind == ports.profile().selection_flags(current, false)
}
