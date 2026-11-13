//! The preparation of one exposure (0xf61314).

use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::focus_reuse;
use crate::adjust::scene_policy::{Treatment, TreatmentInputs};
use crate::exposure::context::{Binding, ExposureContext};
use crate::exposure::ports::Ports;
use crate::exposure::program::Limits;
use crate::exposure::{adjustment, evaluation, snapshot, tables};
use crate::stock::dump;

/// Whether the record's scene admits a subject treatment, and whether it admits the first
/// (0xfa672c and 0xfa66d4 over the bound subjects).
pub(crate) fn treatments(ports: &mut impl Ports, context: &ExposureContext) -> (bool, bool) {
    let binding = &context.binding;
    let record = binding.record_handle();
    let mut profile = ports.profile();
    let inputs = TreatmentInputs {
        active: u32::from(profile.read::<u8, _>(record, &group_00::AT_005D)),
        subjects: binding.subjects.count & 0xff,
        measurement_selection: profile.measurement_selection(record),
        scene: profile.selection_kind(record, binding.refinement_pass),
        class: u32::from(profile.header(record, SharedHeaderByte::At47)),
        callback_armed: u32::from(profile.header(record, SharedHeaderByte::At63)),
        held: u32::from(profile.header(record, SharedHeaderByte::At61)),
        single_subject: binding.subjects.entries[0].detail[0x22],
    };
    (inputs.admits_any(), inputs.admits(Treatment::First))
}

/// Binds the exposure and, for a primary exposure mode, replaces the last exposure's results.
/// The program and the saved tables carry over.
pub fn prepare(ports: &mut impl Ports, limits: &mut impl Limits, context: &mut ExposureContext, binding: Binding) {
    context.binding = binding;
    if !binding.exposure_mode.is_primary() {
        return;
    }
    context.levels = Default::default();
    context.span = Default::default();
    context.areas = Default::default();
    context.selection = Default::default();
    context.evaluation = Default::default();
    context.correction = Default::default();
    context.adjustment = Default::default();
    let (treated, first_treatment) = treatments(ports, context);
    context.outcome.treated = treated;
    context.outcome.first_treatment = first_treatment;
    if let Some(frame) = binding.frame {
        ports.write_u32(dump::section_tag(frame), dump::SECTION_TAG);
        ports.write_u32(dump::end_tag(frame), dump::END_TAG);
    }
    context.snapshot = snapshot::take(ports, binding.snapshot_handle(), binding.lens_reference);
    tables::prepare(ports, context);
    let reused = focus_reuse::reuse_focus_result(ports, context);
    context.outcome.focus_reused = reused;
    if let Some(frame) = binding.frame {
        ports.write_u32(dump::export(frame), u32::from(reused));
    }
    if context.outcome.adjusting {
        adjustment::set_up(ports, context);
    } else {
        evaluation::set_up(ports, limits, context);
    }
}
