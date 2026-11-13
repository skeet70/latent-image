//! The exposure correction calc computes after the evaluation (calc 0x3c01): the request's inputs
//! from the record and the evaluation (0xf68db8).

use xm1_saclac_interface::exposure::CorrectionRequest;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::rate_policy;
use crate::adjust::scene_settings::{self, SceneSelection};
use crate::exposure::context::{ExposureContext, SubjectList};
use crate::exposure::ports::{InPlace, Ports};
use crate::exposure::settings;
use crate::stock::property::{self, RateSetting};
use crate::stock::{lists, shared};

/// Which subject list and summary the correction names; the adjustment path names the snapshot's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubjectSource {
    Bound,
    Snapshot,
}

const fn selection_byte(selection: MeasurementSelection) -> u32 {
    match selection {
        MeasurementSelection::Unselected => 0,
        MeasurementSelection::Record => 1,
        MeasurementSelection::Other(byte) => byte as u32,
    }
}

/// The rate setting a record's selection chooses (0xf5ecc0).
const fn rate_setting(kind: u32, flags: u16, style: u8, auxiliary: u8) -> RateSetting {
    if matches!(style, 4 | 5 | 7) {
        RateSetting::Styled
    } else if matches!(kind, 11..=13) {
        if flags & 2 != 0 { RateSetting::FlaggedKinds } else { RateSetting::Kinds }
    } else {
        match auxiliary {
            0 => RateSetting::Default,
            2 => RateSetting::AuxiliaryTwo,
            _ => RateSetting::Auxiliary,
        }
    }
}

/// The record's two ratio words (0xf5206c).
fn ratio(ports: &mut impl Ports, record: xm1_saclac_interface::RecordHandle) -> [u32; 2] {
    match ports.profile().header(record, SharedHeaderByte::At47) {
        2 | 5 => {
            let bytes: [u8; 8] = ports.profile().read(record, &group_00::AT_0004);
            [u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]])]
        }
        0 => {
            let [low, high] = ports.profile().read(record, &group_00::AT_000C).to_le_bytes();
            [u32::from(low), u32::from(high)]
        }
        _ => [3, 3],
    }
}

/// 0x772ff8's band 2.
fn in_second_step(ports: &mut impl Ports, level: i16) -> bool {
    level != 0 && level >= settings::level_step(ports, 2) && level < settings::level_step(ports, 3)
}

/// The first subject's rate term (0xf69058..0xf690ac).
fn rate(ports: &mut impl Ports, context: &ExposureContext, subjects: &SubjectList, second_factor: bool) -> u32 {
    if !context.outcome.first_treatment || ports.property(property::CORRECTION_RATE) == 0 {
        return 1;
    }
    let detail = &subjects.entries[0].detail;
    let amount = u16::from_le_bytes([detail[0x1e], detail[0x1f]]);
    match rate_policy::drive_rate(ports, second_factor, context.binding.lens_reference as i32, u32::from(amount)).scaled {
        0 => 1,
        scaled => 10_000 / scaled,
    }
}

/// `reference` is the evaluation's reference value plus 250.
pub fn request(ports: &mut impl Ports, context: &ExposureContext, source: SubjectSource, reference: i16) -> CorrectionRequest {
    let binding = &context.binding;
    let record = binding.record_handle();
    let refinement_pass = binding.refinement_pass;
    let levels = context.levels;
    let (subjects, subjects_at, summary_at) = match source {
        SubjectSource::Bound => (&binding.subjects, InPlace::BoundSubjects, InPlace::BoundSummary),
        SubjectSource::Snapshot => (&context.snapshot.subjects, InPlace::SnapshotSubjects, InPlace::SnapshotTables),
    };
    let selection = ports.profile().measurement_selection(record);
    let kind = ports.profile().selection_kind(record, refinement_pass);
    let flags = ports.profile().selection_flags(record, refinement_pass);
    let style = ports.profile().selection_style(record, refinement_pass);
    let auxiliary = ports.profile().selection_auxiliary(record, refinement_pass);
    let header_61 = ports.profile().header(record, SharedHeaderByte::At61);
    let header_47 = ports.profile().header(record, SharedHeaderByte::At47);
    let style_byte: u8 = ports.profile().read(record, &group_00::AT_0043);
    let start_above_target = levels.start.wrapping_sub(levels.target);
    let image_word = if ports.profile().read::<u8, _>(record, &group_00::AT_0041) == 1 { ports.second_report_word() } else { 0 };
    CorrectionRequest {
        style: if matches!(style_byte, 1 | 2) { u32::from(style_byte) } else { 0 },
        measurement_selection: selection_byte(selection),
        selection_kind: kind,
        selection_flags: flags,
        record_kind: u32::from(ports.profile().read::<u8, _>(record, &group_00::AT_0034)),
        record_44: u32::from(ports.profile().read::<u8, _>(record, &group_00::AT_0044)),
        focus_class: u32::from(ports.profile().read::<u8, _>(record, &group_00::AT_004B)),
        record_32: u32::from(ports.profile().read::<u8, _>(record, &group_00::AT_0032)),
        record_3e: u32::from(ports.profile().read::<u8, _>(record, &group_00::AT_003E)),
        variant: u32::from(ports.profile().read::<u8, _>(record, &group_00::AT_0066)),
        lens_reference: binding.lens_reference,
        selection_auxiliary: u32::from(auxiliary),
        image_word,
        rate: rate(ports, context, subjects, header_61 != 0),
        list_words: lists::CORRECTION_WORDS.map(|at| ports.read_u32(at)),
        list_byte: ports.read_u8(lists::CORRECTION_BYTE),
        list_word: ports.read_u32(lists::CORRECTION_WORD),
        level_curves: core::array::from_fn(|curve| Some(shared::level_curve_base(curve))),
        start: levels.start as u16,
        start_above_target: start_above_target as u16,
        reference_offset: ports.reference_offset(binding.lens_reference) as u16,
        curve_table: scene_settings::curve_table(SceneSelection { measurement_selection: selection, scene: kind, flags }).0,
        rate_setting: ports.property(property::rate_setting(rate_setting(kind, flags, style, auxiliary))) as u16,
        reference: reference.wrapping_add(start_above_target) as u16,
        ratio: ratio(ports, record),
        second_step: in_second_step(ports, levels.measured as i16),
        treated: context.outcome.treated,
        first_treatment: context.outcome.first_treatment,
        subjects: Some(ports.address(subjects_at)),
        summary: Some(ports.address(summary_at)),
        subject_curves: core::array::from_fn(|curve| Some(shared::subject_curve_base(curve))),
        header_61: u32::from(header_61),
        header_47: u32::from(header_47),
        unmeasured: levels.unmeasured,
        record_20: ports.profile().read(record, &group_00::AT_0020),
        buffers: shared::CORRECTION_BUFFERS.map(Some),
        frame: binding.frame,
    }
}
