//! Whether the focus result the snapshot took can stand for the current record, and its trace
//! in the client's dump frame.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;

use crate::exposure::context::ExposureContext;
use crate::policy::SelectionKind;

/// The current record's values the snapshot's result is matched against.
pub trait ReuseRecord {
    fn selection_kind(&self, record: Option<RecordHandle>) -> SelectionKind;
    fn measurement_selection(&self, record: Option<RecordHandle>) -> u8;
    /// Whether the record's subject count must equal the result's.
    fn compares_subjects(&self, record: Option<RecordHandle>) -> bool;
    fn callback_armed(&self, record: Option<RecordHandle>) -> bool;
    fn scene_kind(&self, record: Option<RecordHandle>) -> u32;
    fn follow_word(&self, record: Option<RecordHandle>) -> u16;
}

pub trait ReuseSettings {
    /// A result whose level is this many tens or more from the limit is not reused.
    fn level_tolerance(&self) -> u8;
}

/// The result's matching values as the dump frame records them.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReuseTrace {
    pub adjustment: [u32; 3],
    pub adjustment_kind: u32,
    pub subject_count: u32,
    pub adjustment_level: i32,
    pub limit: i32,
    pub tolerance: u8,
}

pub trait ReuseDump {
    /// Also clears the frame's out-of-tolerance mark.
    fn trace(&mut self, frame: SharedAddress, trace: &ReuseTrace);
    fn mark_out_of_tolerance(&mut self, frame: SharedAddress);
}

/// Traces the snapshot's result into the bound frame, then says whether it can be reused.
pub fn reuse_focus_result(
    context: &ExposureContext,
    record: &impl ReuseRecord,
    settings: &impl ReuseSettings,
    dump: &mut impl ReuseDump,
) -> bool {
    let focus = &context.snapshot.focus;
    let frame = context.binding.frame;
    if let Some(frame) = frame {
        dump.trace(frame, &ReuseTrace {
            adjustment: focus.adjustment,
            adjustment_kind: u32::from(focus.adjustment_kind),
            subject_count: focus.subject_count,
            adjustment_level: i32::from(focus.adjustment_level) + 500,
            limit: i32::from(focus.limit) + 500,
            tolerance: settings.level_tolerance(),
        });
    }
    let binding = &context.binding;
    if binding.mask & 2 == 0 && binding.mode == 1 {
        return true;
    }
    if focus.adjustment[0] == 0 || !context.outcome.adjusting || context.adjustment.state != 0 {
        return false;
    }
    let distance = (i32::from(focus.limit) - i32::from(focus.adjustment_level)).unsigned_abs() as u16;
    if i32::from(distance) >= i32::from(settings.level_tolerance()) * 10 {
        if let Some(frame) = frame {
            dump.mark_out_of_tolerance(frame);
        }
        return false;
    }
    let current = binding.record;
    let kind = record.selection_kind(current);
    let selected = record.measurement_selection(current) == 1;
    if kind.qualifies(selected, context.levels.unmeasured, binding.option != 0) {
        return false;
    }
    if kind.0 != 6 && binding.option != focus.adjustment[1] {
        return false;
    }
    if record.compares_subjects(current) && binding.subjects.count != focus.subject_count {
        return false;
    }
    if !record.callback_armed(current) {
        return true;
    }
    record.scene_kind(current) == focus.adjustment[2] && focus.adjustment_kind == record.follow_word(current)
}
