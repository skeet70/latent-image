//! The tables calc evaluates an exposure against (0xf6ac90): whether the adjustment path replaces
//! the evaluation, the record's level curves relative to the focus level, and their trace in the
//! client's dump frame.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_13};
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::scene_policy::{scene_enabled, SceneMaskInputs};
use crate::exposure::context::{ExposureContext, FocusResult, SUBJECTS};
use crate::exposure::levels;
use crate::exposure::ports::Ports;
use crate::exposure::settings;
use crate::stock::property::{self, WindowSource};
use crate::stock::{dump, record, shared};

/// The range every curve point is clamped to: the configured floor less 90 steps and the
/// configured ceiling plus 25, both in tenths.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CurveRange {
    pub(crate) lowest: i32,
    pub(crate) highest: i32,
}

impl CurveRange {
    pub(crate) fn read(ports: &mut impl Ports) -> Self {
        let floor = settings::level(ports, property::TABLE_FLOOR).wrapping_mul(10);
        let ceiling = settings::level(ports, property::TABLE_CEILING).wrapping_mul(10);
        Self { lowest: i32::from(floor) - 900, highest: i32::from(ceiling) + 250 }
    }

    fn clamp(self, value: i16) -> u16 { i32::from(value).max(self.lowest).min(self.highest) as u16 }
}

/// The focus level the record's tables are relative to.
fn focus_level(focus: &FocusResult) -> i16 { focus.levels[3].wrapping_add(focus.levels[1]) }

fn read_i16(ports: &mut impl Ports, at: SharedAddress) -> i16 { ports.read_u16(at) as i16 }

/// Prepares the tables for the bound record and decides `outcome.adjusting`.
pub fn prepare(ports: &mut impl Ports, context: &mut ExposureContext) {
    let range = CurveRange::read(ports);
    let frame = context.binding.frame;
    if let Some(frame) = frame {
        let (cleared, bytes) = dump::cleared(frame);
        ports.fill(cleared, bytes, 0);
        let first = ports.property(property::DUMP_FIRST);
        ports.write_u32(dump::configuration(frame, 0), first);
        let second = ports.property(property::DUMP_SECOND);
        ports.write_u32(dump::configuration(frame, 1), second);
        ports.write_u32(dump::subjects_present(frame), u32::from(context.snapshot.subjects.count != 0));
        ports.write_u32(dump::treated(frame), u32::from(context.outcome.treated));
    }
    let record_handle = context.binding.record_handle();
    let refinement_pass = context.binding.refinement_pass;
    let inputs = SceneMaskInputs {
        measurement_selection: ports.profile().measurement_selection(record_handle),
        scene: ports.profile().selection_kind(record_handle, refinement_pass),
        callback_armed: u32::from(ports.profile().header(record_handle, SharedHeaderByte::At63)),
        treated: context.outcome.treated,
        subjects: context.snapshot.subjects.count,
    };
    let enabled = scene_enabled(ports, inputs);
    let focus = context.snapshot.focus;
    let focus_level = focus_level(&focus);
    context.outcome.adjusting = enabled && context.snapshot.valid != 0;
    if context.outcome.adjusting {
        if let Some(frame) = frame {
            trace(ports, frame, &focus, context.snapshot.subjects.count, focus_level);
        }
    }
    let weight = u32::from(focus.weights[0]) + u32::from(focus.weights[1]);
    if weight >= u32::from(settings::word(ports, property::WEIGHT_LIMIT)) {
        context.outcome.adjusting = false;
    }
    if !context.outcome.adjusting {
        return;
    }
    let lens = ports.level(focus.lens_detail, focus.reference);
    let level = focus.weights[4]
        .wrapping_add(focus.levels[0] as u16)
        .wrapping_sub(focus.levels[3] as u16)
        .wrapping_sub(focus.levels[1] as u16)
        .wrapping_sub(focus.levels[2] as u16)
        .wrapping_add(lens as u16) as i16;
    let (upper, lower) = window(ports, record_handle);
    context.outcome.adjusting = upper > level && level > lower;
    if !context.outcome.adjusting {
        return;
    }
    context.adjustment.state = adjustment_state(ports, &focus);
    context.levels = levels::start(ports, record_handle, true);
    let target = context.levels.target;
    for curve in 0..shared::LEVEL_CURVES {
        for point in 0..shared::CURVE_POINTS {
            let value = read_i16(ports, record::level_table(focus.level_tables, curve, point));
            ports.write_u16(shared::level_curve(curve, point), range.clamp(target.wrapping_sub(focus_level).wrapping_add(value)));
        }
        for point in 0..shared::SUBJECT_POINTS {
            let value = read_i16(ports, record::subject_level(focus.subject_levels, curve, point));
            ports.write_u16(shared::subject_curve(curve, point), range.clamp(target.wrapping_sub(focus_level).wrapping_add(value)));
        }
    }
    context.adjustment.level = level.wrapping_add(target) as u16;
    for point in 0..shared::CURVE_POINTS {
        let class = match ports.read_u8(record::level_class(focus.level_classes, point)) {
            3 => 1,
            4 => 2,
            class => class,
        };
        ports.write_u8(shared::level_class(point), class);
    }
    for flag in 0..shared::SELECTION_FLAG_BYTES {
        ports.write_u8(SharedAddress(shared::SELECTION_FLAGS.0 + flag as u32), 0);
    }
}

/// The record's tables relative to the focus level, its lens and focus levels and its weights.
fn trace(ports: &mut impl Ports, frame: SharedAddress, focus: &FocusResult, subjects: u32, focus_level: i16) {
    let relative = |value: i16| (i32::from(value) - i32::from(focus_level) + 500) as u32;
    ports.write_u32(dump::level_bias(frame), i32::from(focus.level_bias) as u32);
    for point in 0..shared::CURVE_POINTS {
        for table in 0..shared::LEVEL_CURVES {
            let value = read_i16(ports, record::level_table(focus.level_tables, table, point));
            ports.write_u32(dump::level_table(frame, table, point), relative(value));
        }
        let class = ports.read_u8(record::level_class(focus.level_classes, point)) as i8;
        ports.write_u32(dump::level_class(frame, point), i32::from(class) as u32);
    }
    for subject in 0..(subjects as u16).min(SUBJECTS as u16) as usize {
        let value = read_i16(ports, record::subject_level(focus.subject_levels, 0, subject));
        ports.write_u32(dump::subject_level(frame, subject), relative(value));
    }
    ports.write_u32(dump::lens_detail(frame), focus.lens_detail);
    let base = ports.base_level(focus.lens_detail);
    ports.write_u32(dump::lens_base_level(frame), i32::from(base) as u32);
    for (index, level) in focus.levels[..4].iter().enumerate() {
        ports.write_u32(dump::focus_level(frame, index), i32::from(*level) as u32);
    }
    for (index, weight) in focus.weights[..4].iter().enumerate() {
        ports.write_u32(dump::weight(frame, index), u32::from(*weight));
    }
}

/// The adjustment window's upper and lower bounds in tenths, less step 2 of the level table and 200.
fn window(ports: &mut impl Ports, record_handle: RecordHandle) -> (i16, i16) {
    let selection = ports.profile().measurement_selection(record_handle);
    let alternate = ports.profile().read(record_handle, &group_13::AT_20D4);
    let kind = ports.profile().read(record_handle, &group_00::AT_0034);
    let base = settings::level_step(ports, 2).wrapping_add(200);
    let source = match (selection, kind) {
        (MeasurementSelection::Record, 6 | 7) => WindowSource::RecordExtended,
        (MeasurementSelection::Record, _) => WindowSource::Record,
        _ if alternate == 1 => WindowSource::Alternate,
        _ => WindowSource::Default,
    };
    let (upper, lower) = property::window(source);
    let upper = settings::level(ports, upper).wrapping_mul(10).wrapping_sub(base);
    let lower = settings::level(ports, lower).wrapping_mul(10).wrapping_sub(base);
    (upper, lower)
}

/// 3 when the focus result carries its fifth level, else from the third and fourth weights
/// against their limits: 2 for the third alone, 1 for the fourth alone, 3 for both.
fn adjustment_state(ports: &mut impl Ports, focus: &FocusResult) -> u32 {
    if focus.levels[4] != 0 {
        return 3;
    }
    let third = focus.weights[2] > settings::word(ports, property::SECOND_WEIGHT_LIMIT);
    let fourth = focus.weights[3] > settings::word(ports, property::THIRD_WEIGHT_LIMIT);
    match (third, fourth) {
        (true, true) => 3,
        (false, true) => 1,
        (true, false) => 2,
        (false, false) => 0,
    }
}
