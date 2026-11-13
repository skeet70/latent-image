//! The preparation of one exposure (0xf61314).

use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::scene_policy::{Treatment, TreatmentInputs};
use crate::exposure::context::ExposureContext;
use crate::exposure::ports::Ports;

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
