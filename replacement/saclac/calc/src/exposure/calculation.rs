//! The input calc 0x3c02 calculates from: the exposure's levels, the record's selection and
//! metering values and the shot's plan (0xf61b8c, 0xf6191c, 0xf61754).

use xm1_saclac_interface::exposure::{CalculationInput, INPUT_LEVELS};
use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_01, group_13, group_31};
use xm1_saclac_profile_records::stock::shared::{group_20, group_27, group_37};
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::result_dispatch;
use crate::adjust::scene_settings::{self, EnableInputs, SceneSelection};
use crate::capture_requests::capture_offset;
use crate::exposure::context::ExposureContext;
use crate::exposure::levels;
use crate::exposure::ports::{Lens, Ports};
use crate::profile::selection_code;

/// The target word a new input carries until a half press takes the retained targets (0xf61bf4).
const NEW_TARGET_WORD: u32 = 0x80;
/// The grid of a half press whose record selects no measurement (0xf61d78).
const DEFAULT_GRID: [u8; 2] = [3, 3];
/// The level that stands for a record without its fifth level (0xf61840).
const NO_LEVEL: i16 = 0x7fff;

/// What the shot's selection plan contributes to an input.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlanInput {
    /// The plan's first word.
    pub kind: u32,
    /// The five correction halfwords the plan keeps.
    pub exposure: [u16; 5],
}

/// The record's base level relative to the lens.
fn lens_base(ports: &mut impl Ports, record: RecordHandle) -> u16 {
    let (reference, first, second) = {
        let profile = ports.profile();
        (profile.read(record, &group_01::AT_00BC), profile.read::<i16, _>(record, &group_01::AT_00B8), profile.read::<i16, _>(record, &group_01::AT_00BA))
    };
    (first.wrapping_add(second) as u16).wrapping_sub(ports.reference_offset(reference) as u16)
}

/// The record's metered level relative to the lens, and the metered level over the base.
fn metered(ports: &mut impl Ports, record: RecordHandle) -> (i16, u16) {
    let (level, reference) = {
        let profile = ports.profile();
        let level = match profile.read::<u8, _>(record, &group_00::AT_0043) {
            1 => &group_01::AT_0094,
            2 => &group_01::AT_0092,
            _ => &group_01::AT_00A8,
        };
        (profile.read::<i16, _>(record, level) as u16, profile.read(record, &group_01::AT_00BC))
    };
    let metered = level.wrapping_sub(ports.reference_offset(reference) as u16);
    (metered as i16, level.wrapping_add(lens_base(ports, record)))
}

/// The record's levels relative to the lens.
fn record_levels(ports: &mut impl Ports, record: RecordHandle, refinement_pass: bool) -> [i16; INPUT_LEVELS] {
    let base = lens_base(ports, record);
    let profile = ports.profile();
    let fields = [
        &group_01::AT_00AE,
        &group_01::AT_00AC,
        &group_01::AT_00A4,
        &group_01::AT_00A6,
        &group_01::AT_00AA,
        &group_01::AT_00AC,
        &group_01::AT_00AE,
        &group_01::AT_00B0,
        &group_01::AT_00A4,
    ];
    let mut levels = fields.map(|field| base.wrapping_add(profile.read::<i16, _>(record, field) as u16) as i16);
    if profile.read::<u32, _>(record, &group_01::AT_00C4) == 0 {
        levels[4] = NO_LEVEL;
    }
    let alternative: i16 = profile.read(record, &group_01::AT_00B2);
    let mut profile = ports.profile();
    let inputs = EnableInputs {
        selection: SceneSelection {
            measurement_selection: profile.measurement_selection(record),
            scene: profile.selection_kind(record, refinement_pass),
            flags: profile.selection_flags(record, refinement_pass),
        },
        auxiliary: profile.selection_auxiliary(record, refinement_pass),
        hold: profile.read(record, &group_00::AT_0014),
        selector: profile.read(record, &group_00::AT_0051),
    };
    if scene_settings::adjustment_enabled(ports, inputs) {
        levels[8] = base.wrapping_add(alternative as u16) as i16;
    }
    levels
}

/// The record's selection grid and its steps.
fn grid(ports: &mut impl Ports, record: RecordHandle, live: bool) -> ([u8; 2], [u8; 2]) {
    let selected = ports.profile().measurement_selection(record) == MeasurementSelection::Record;
    if !live && !selected {
        return (DEFAULT_GRID, [0; 2]);
    }
    let steps = if selected {
        let layout = ports.profile().read(record, &group_00::AT_003D);
        ports.grid_steps(layout)
    } else {
        [0; 2]
    };
    (steps.map(|step| step.wrapping_add(1)), steps)
}

/// Builds the input for publication in the shared calc buffer and keeps its metered level.
pub fn build_input(ports: &mut impl Ports, context: &mut ExposureContext, area: [u32; 2], plan: PlanInput, selection: bool) -> CalculationInput {
    let binding = context.binding;
    let record = binding.snapshot_handle();
    let refinement_pass = binding.refinement_pass;
    let (metered_level, metering_level) = metered(ports, record);
    let (grid, grid_steps) = grid(ports, record, binding.live);
    let levels = record_levels(ports, record, refinement_pass);
    let record_level = levels::measured_level(ports, record);
    let capture_offset = capture_offset(ports, record);
    let second_result = result_dispatch::second_result(ports, record).code();
    let mut profile = ports.profile();
    let start = context.levels;
    let input = CalculationInput {
        selection,
        lens_reference: binding.lens_reference,
        target_word: NEW_TARGET_WORD,
        target_half: 0,
        area,
        target_level: start.target,
        start_level: start.start,
        measured_level: start.measured,
        level_profile: start.curve,
        unmeasured: start.unmeasured,
        level_policy: start.policy,
        refinement_pass,
        first_treatment: context.outcome.first_treatment,
        metering_level,
        grid,
        grid_steps,
        plan_kind: plan.kind,
        plan_exposure: plan.exposure,
        measurement_selection: u32::from(selection_code(profile.measurement_selection(record))),
        selection_kind: profile.selection_kind(record, refinement_pass),
        selection_flags: profile.selection_flags(record, refinement_pass),
        selection_style: u32::from(profile.selection_style(record, refinement_pass)),
        selection_auxiliary: u32::from(profile.selection_auxiliary(record, refinement_pass)),
        record_14: profile.read(record, &group_00::AT_0014),
        record_20: profile.read(record, &group_00::AT_0020),
        record_24: profile.read(record, &group_00::AT_0024),
        record_26: profile.read(record, &group_00::AT_0026),
        record_32: u32::from(profile.read::<u8, _>(record, &group_00::AT_0032)),
        record_33: profile.read(record, &group_00::AT_0033),
        record_34: u32::from(profile.read::<u8, _>(record, &group_00::AT_0034)),
        record_3d: u32::from(profile.read::<u8, _>(record, &group_00::AT_003D)),
        record_3e: u32::from(profile.read::<u8, _>(record, &group_00::AT_003E)),
        record_43: u32::from(profile.read::<u8, _>(record, &group_00::AT_0043)),
        record_45: u32::from(profile.read::<u8, _>(record, &group_00::AT_0045)),
        record_4b: u32::from(profile.read::<u8, _>(record, &group_00::AT_004B)),
        record_51: u32::from(profile.read::<u8, _>(record, &group_00::AT_0051)),
        record_5b: u32::from(profile.read::<u8, _>(record, &group_00::AT_005B)),
        record_5d: u32::from(profile.read::<u8, _>(record, &group_00::AT_005D)),
        record_5f: u32::from(profile.read::<u8, _>(record, &group_00::AT_005F)),
        record_66: u32::from(profile.read::<u8, _>(record, &group_00::AT_0066)),
        record_67: u32::from(profile.read::<u8, _>(record, &group_00::AT_0067)),
        record_68: u32::from(profile.read::<u8, _>(record, &group_00::AT_0068)),
        record_69: u32::from(profile.read::<u8, _>(record, &group_00::AT_0069)),
        record_20b0: profile.read(record, &group_13::AT_20B0_4),
        record_20c8: profile.read(record, &group_13::AT_20C8),
        record_20cc: profile.read(record, &group_13::AT_20CC),
        record_20d4: profile.read(record, &group_13::AT_20D4),
        record_2110: profile.read(record, &group_31::AT_2110),
        offset: profile.read(record, &group_00::AT_001C),
        lead: profile.read(record, &group_00::AT_001E),
        source: u32::from(profile.read::<u8, _>(record, &group_00::AT_0044)),
        record_level,
        header_47: u32::from(profile.header(record, SharedHeaderByte::At47)),
        header_61: u32::from(profile.header(record, SharedHeaderByte::At61)),
        header_63: u32::from(profile.header(record, SharedHeaderByte::At63)),
        interval_tracking: u32::from(profile.read(record, &group_00::INTERVAL_TRACKING).0),
        capture_offset,
        offered_measurements: profile.read_shared(record, &group_20::OFFERED_MEASUREMENTS).0,
        window_status: profile.read_shared(record, &group_27::WINDOW_STATUS).0,
        shared_738: profile.read_shared(record, &group_27::AT_0738),
        shared_85c: {
            let bytes = profile.read_shared(record, &group_37::AT_085C);
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        },
        second_result,
        grid_layout: u32::from(profile.grid_layout(record)),
        record_lens_reference: profile.read(record, &group_01::AT_00BC),
        levels,
        option: binding.option,
    };
    context.metered_level = metered_level;
    input
}

/// Takes back an input saved for a resumed half press, its levels moved from the lens reference it
/// was built at to `lens_reference` (0xf1fc5c).
pub fn resume_input(lens: &mut impl Lens, saved: &CalculationInput, lens_reference: u32) -> CalculationInput {
    let shift = lens.reference_offset(saved.record_lens_reference).wrapping_sub(lens.reference_offset(lens_reference));
    let mut input = *saved;
    input.metering_level = input.metering_level.wrapping_add(shift as u16);
    for level in &mut input.levels {
        *level = level.wrapping_add(shift);
    }
    input.lens_reference = lens_reference;
    input.record_lens_reference = lens_reference;
    input
}
