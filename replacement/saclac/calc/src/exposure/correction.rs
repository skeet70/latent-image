//! The exposure correction calc computes after the evaluation (calc 0x3c01): the request's inputs
//! from the record and the evaluation (0xf68db8), the tables refreshed before it is requested
//! (0xf6a330) and the request itself (0xf5ff88).

use xm1_saclac_interface::calc::{CalcPayload, SharedAddress};
use xm1_saclac_interface::exposure::CorrectionRequest;
use xm1_saclac_interface::stock::StockCallbackWord;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::rate_policy;
use crate::adjust::scene_settings::{self, SceneSelection};
use crate::exposure::context::{ExposureContext, SubjectList};
use crate::exposure::evaluation::reference_value;
use crate::exposure::levels;
use crate::exposure::ports::{InPlace, Ports, Property};
use crate::exposure::prepare::treatments;
use crate::exposure::settings;
use crate::exposure::tables::CurveRange;
use crate::policy::raise_all;
use crate::requests::Peers;
use crate::stock::property::{self, RateSetting};
use crate::stock::shared::{self, CURVE_POINTS, LEVEL_CURVES, SUBJECT_POINTS};
use crate::stock::lists;

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

/// The half-press mode whose correction keeps the tables as they are (0xf5ffa0).
const KEEP_TABLES_MODE: u32 = 7;

fn read_i16(ports: &mut impl Ports, at: SharedAddress) -> i16 { ports.read_u16(at) as i16 }

/// The four level curves, then the four subject rows, point by point.
fn table_points() -> impl Iterator<Item = SharedAddress> {
    let curves = (0..LEVEL_CURVES).flat_map(|curve| (0..CURVE_POINTS).map(move |point| shared::level_curve(curve, point)));
    let subjects = (0..LEVEL_CURVES).flat_map(|row| (0..SUBJECT_POINTS).map(move |point| shared::subject_curve(row, point)));
    curves.chain(subjects)
}

/// 0xf5387c over every table.
fn raise_tables(ports: &mut impl Ports, offset: i16, limit: i16) {
    if offset == 0 {
        return;
    }
    for at in table_points() {
        let mut value = read_i16(ports, at);
        raise_all(core::slice::from_mut(&mut value), offset, i32::from(limit));
        ports.write_u16(at, value as u16);
    }
}

/// A signed configuration byte in tenths.
fn signed_tenths(ports: &mut impl Ports, id: Property) -> i16 { i16::from(ports.property(id) as u8 as i8).wrapping_mul(10) }

/// Clamps every table to a window around the adjustment level. Each side is the near bound, or
/// the margin past the level when the weighted count of curve 0's points beyond that bound
/// exceeds its limit; the configured table range bounds both (0xf6a350..0xf6a6d4).
fn narrow_to_window(ports: &mut impl Ports, context: &ExposureContext) {
    let range = CurveRange::read(ports);
    let margin = settings::level(ports, property::WINDOW_MARGIN).wrapping_mul(10);
    let near_above = signed_tenths(ports, property::NEAR_ABOVE);
    let far_above = signed_tenths(ports, property::FAR_ABOVE);
    let near_below = signed_tenths(ports, property::NEAR_BELOW);
    let far_below = signed_tenths(ports, property::FAR_BELOW);
    let weights = [property::NEAR_ABOVE_WEIGHT, property::FAR_ABOVE_WEIGHT, property::NEAR_BELOW_WEIGHT, property::FAR_BELOW_WEIGHT].map(|id| ports.property(id));
    let fifth = context.snapshot.focus.levels[4];
    let raised = context.adjustment.level.wrapping_add(fifth as u16);
    let level = if fifth > 99 { raised.wrapping_sub(100) } else { raised } as i16;
    let mut counts = [0u32; 4];
    for point in 0..CURVE_POINTS {
        let distance = read_i16(ports, shared::level_curve(0, point)).wrapping_sub(level);
        let band = if near_below <= distance && distance <= near_above {
            continue;
        } else if near_above < distance && distance <= far_above {
            0
        } else if far_above < distance {
            1
        } else if far_below <= distance && distance < near_below {
            2
        } else {
            3
        };
        counts[band] += 1;
    }
    let weighted = |near: usize| weights[near].wrapping_mul(counts[near]).wrapping_add(weights[near + 1].wrapping_mul(counts[near + 1])) as u16;
    let upper = if weighted(0) > settings::word(ports, property::SECOND_WEIGHT_LIMIT) { level.wrapping_add(margin).wrapping_add(250) } else { level.wrapping_add(near_above) };
    let upper = if i32::from(upper) > range.highest { range.highest as i16 } else { upper };
    let lower = if weighted(2) > settings::word(ports, property::THIRD_WEIGHT_LIMIT) { level.wrapping_sub(900).wrapping_sub(margin) } else { level.wrapping_add(near_below) };
    let lower = if i32::from(lower) < range.lowest { range.lowest as i16 } else { lower };
    for at in table_points() {
        let value = read_i16(ports, at);
        let narrowed = value.min(upper).max(lower);
        if narrowed != value {
            ports.write_u16(at, narrowed as u16);
        }
    }
}

fn save_tables(ports: &mut impl Ports, context: &mut ExposureContext) {
    let saved = &mut context.saved;
    for (curve, points) in saved.curves.iter_mut().enumerate() {
        for (point, value) in points.iter_mut().enumerate() {
            *value = ports.read_u16(shared::level_curve(curve, point));
        }
    }
    for (row, points) in saved.subjects.iter_mut().enumerate() {
        for (point, value) in points.iter_mut().enumerate() {
            *value = ports.read_u16(shared::subject_curve(row, point));
        }
    }
    saved.start = context.levels.start;
}

fn restore_tables(ports: &mut impl Ports, context: &ExposureContext) {
    let saved = &context.saved;
    for (curve, points) in saved.curves.iter().enumerate() {
        for (point, value) in points.iter().enumerate() {
            ports.write_u16(shared::level_curve(curve, point), *value);
        }
    }
    for (row, points) in saved.subjects.iter().enumerate() {
        for (point, value) in points.iter().enumerate() {
            ports.write_u16(shared::subject_curve(row, point), *value);
        }
    }
}

/// Brings the tables to the correction's start level and, for a record with header byte 0x63,
/// carries them between primary exposures and the next exposure of another mode, then rebuilds
/// the treatments and the correction request (0xf6a330).
pub fn refresh(ports: &mut impl Ports, context: &mut ExposureContext) {
    let primary = context.binding.exposure_mode.is_primary();
    if primary {
        if context.outcome.adjusting && context.snapshot.focus.levels[4] != 0 {
            narrow_to_window(ports, context);
        }
        raise_tables(ports, context.correction.start_above_target as i16, context.correction.reference as i16);
    }
    let record = context.binding.record_handle();
    if ports.profile().header(record, SharedHeaderByte::At63) == 0 {
        return;
    }
    if primary {
        save_tables(ports, context);
    } else {
        restore_tables(ports, context);
        context.levels = levels::start(ports, record, context.outcome.adjusting);
        let levels = context.levels;
        let offset = levels.start.wrapping_sub(context.saved.start);
        let limit = levels.start.wrapping_add(250).wrapping_sub(levels.target).wrapping_add(reference_value(ports, context));
        raise_tables(ports, offset, limit);
    }
    (context.outcome.treated, context.outcome.first_treatment) = treatments(ports, context);
    let source = if context.outcome.adjusting && context.binding.subjects.count != 0 { SubjectSource::Snapshot } else { SubjectSource::Bound };
    let reference = reference_value(ports, context).wrapping_add(250);
    context.correction = request(ports, context, source, reference);
}

/// Whether calc was asked for the correction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CorrectionPost {
    Posted,
    /// The caller completes the correction without a result.
    Skipped,
}

/// Refreshes the tables, then posts calc 0x3c01 or skips it (0xf5ff88).
pub fn post<P: Peers>(ports: &mut impl Ports, peers: &mut P, context: &mut ExposureContext, callback: StockCallbackWord) -> Result<CorrectionPost, P::Error> {
    if context.binding.mode != KEEP_TABLES_MODE {
        refresh(ports, context);
    }
    let reused = context.outcome.focus_reused && !context.binding.exposure_mode.uses_adjustment() && ports.property(property::CORRECTION_FLAGS) & 2 == 0;
    if reused || context.binding.mask & 1 == 0 {
        return Ok(CorrectionPost::Skipped);
    }
    peers.calc(&CalcPayload::Correction { request: context.correction, callback })?;
    Ok(CorrectionPost::Posted)
}
