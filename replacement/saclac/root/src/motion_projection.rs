#![forbid(unsafe_code)]

use xm1_saclac_calc::adjust::motion::{exposure::FrameExposure, plan::{FocusResult, Plan}, ports::AreaGrid, state::MotionState};
use xm1_shot_sequence::pipeline_work::{CaptureBlock, LensBlock, ScanBlock, SubjectBlock};

fn word(bytes: &mut [u8], at: usize, value: u32) { bytes[at..at + 4].copy_from_slice(&value.to_le_bytes()); }
fn half(bytes: &mut [u8], at: usize, value: u16) { bytes[at..at + 2].copy_from_slice(&value.to_le_bytes()); }

pub(crate) fn subject(exposure: &FrameExposure) -> SubjectBlock {
    let mut block = SubjectBlock::default();
    block.0[0] = exposure.step;
    for (at, value) in [(2, exposure.base_level), (4, exposure.level), (6, exposure.at_06), (8, exposure.change), (0xa, exposure.exposure_level), (0xc, exposure.at_0c), (0xe, exposure.profile_offset)] { half(&mut block.0, at, value as u16); }
    for (at, value) in [(0x10, exposure.lens_step), (0x14, exposure.profile), (0x1c, exposure.time), (0x28, exposure.gain)] { word(&mut block.0, at, value); }
    half(&mut block.0, 0x18, exposure.change_scale);
    half(&mut block.0, 0x1a, exposure.gain_scale);
    block.0[0x2c] = exposure.frames;
    block.0[0x2d] = exposure.mode;
    half(&mut block.0, 0x2e, exposure.first_level as u16);
    block
}

pub(crate) fn capture(state: &MotionState) -> CaptureBlock {
    let readings = state.readings();
    let mut block = CaptureBlock::default();
    for (at, value) in [
        (4, u32::from(xm1_saclac_calc::profile::selection_code(readings.measurement_selection))),
        (8, u32::from(readings.scene)), (0x18, u32::from(readings.class)), (0x20, readings.lens),
        (0x2c, u32::from(readings.at_0035)), (0x30, u32::from(readings.at_0036)), (0x34, u32::from(readings.at_0038)),
        (0x38, readings.at_20cc), (0x40, readings.at_20c8), (0x44, readings.at_20d0),
        (0x4c, readings.selection_value), (0x50, readings.at_0164), (0x54, readings.at_0168),
        (0xb8, readings.locked), (0xe0, u32::from(readings.armed)), (0xf0, readings.drive_kind.0), (0xf4, readings.reference),
        (0xfc, u32::from(readings.at_003c)), (0x104, u32::from(readings.at_0039)), (0x108, u32::from(readings.at_003a)),
        (0x10c, u32::from(readings.at_003b)), (0x138, readings.at_0194.0),
        (0x140, u32::from(readings.record_3e)),
    ] { word(&mut block.0, at, value); }
    half(&mut block.0, 0x48, readings.row_scale[0]); half(&mut block.0, 0x4a, readings.row_scale[1]);
    block.0[0xbe] = readings.compares_subjects;
    block.0[0x13c] = readings.at_0194.1;
    block
}

fn grid(bytes: &mut [u8], at: usize, grid: &AreaGrid) {
    for (offset, value) in [(0, grid.cell_height), (4, grid.cell_width), (8, grid.left), (12, grid.top)] { word(bytes, at + offset, value); }
    half(bytes, at + 16, grid.gap_horizontal); half(bytes, at + 18, grid.gap_vertical);
    bytes[at + 20] = grid.columns; bytes[at + 21] = grid.rows;
    bytes[at + 22..at + 24].copy_from_slice(&grid.at_16);
}

pub(crate) fn lens(state: &MotionState) -> LensBlock {
    let search = state.search(); let layout = state.layout();
    let mut block = LensBlock::default();
    word(&mut block.0, 0, search.mode.code());
    if let Some(tracking) = layout.tracking {
        let area = tracking.area;
        for (offset, value) in [area.centre_y, area.centre_x, area.y, area.x, area.height, area.width, tracking.class.code(), 1].into_iter().enumerate() { word(&mut block.0, 0x14 + 4 * offset, value); }
    }
    let window = layout.crop.window;
    for (at, value) in [(0x38, window.left), (0x3c, window.top), (0x40, window.height), (0x44, window.width), (0x80, layout.curve_limit)] { word(&mut block.0, at, value); }
    for (i, value) in layout.curve.into_iter().enumerate() { half(&mut block.0, 0x78 + 2 * i, value); }
    half(&mut block.0, 0x84, layout.exposure_ratio);
    word(&mut block.0, 0x58, u32::from(layout.step_setting));
    word(&mut block.0, 0x4a8, search.channel_class.code());
    word(&mut block.0, 0x4ac, u32::from(search.retry_missing));
    grid(&mut block.0, 0x4b0, &layout.grids[0]); grid(&mut block.0, 0x4c8, &layout.grids[1]);
    block
}

pub(crate) fn scan(plan: &Plan) -> ScanBlock {
    let mut block = ScanBlock::default();
    block.0[0] = plan.decided.0;
    for (at, value) in [
        (4, plan.motion.code()), (8, u32::from(plan.kind.0)), (0xc, u32::from(plan.profile.reverse)),
        (0x18, plan.speed), (0x20, u32::from(plan.speed_choice)),
        (0x30, u32::from(plan.checks_speed)), (0x34, u32::from(plan.channel_window)),
        (0x38, u32::from(plan.checks_peak)), (0x3c, u32::from(plan.window_enabled)),
        (0x40, u32::from(plan.below_level)), (0x44, u32::from(plan.untreated_window)),
        (0x48, u32::from(plan.single_cell_window)), (0x4c, plan.layout.code()),
        (0x50, plan.primary_share), (0x54, plan.secondary_share), (0x94, plan.decision),
        (0x168, u32::from(plan.flags.found)), (0x16c, u32::from(plan.flags.searching)), (0x170, plan.flags.side),
        (0x174, plan.tone_limits[0]), (0x178, plan.tone_limits[1]), (0x17c, plan.scan_requested), (0x180, plan.selection),
    ] { word(&mut block.0, at, value); }
    for (at, value) in [(0x10, plan.profile.start as u16), (0x12, plan.profile.end as u16), (0x14, plan.last_frame), (0x16, plan.step as u16), (0x1c, plan.frame_travel), (0x1e, plan.frame_depth), (0x90, plan.first_frame), (0x98, plan.near_bound as u16)] { half(&mut block.0, at, value); }
    for (i, value) in plan.speeds.into_iter().enumerate() { word(&mut block.0, 0x24 + 4 * i, value); }
    block.0[0x58..0x5b].copy_from_slice(&plan.weights);
    for (i, value) in plan.tone.words.into_iter().enumerate() { word(&mut block.0, 0x9c + 4 * i, value); }
    let result = &plan.result;
    for (i, value) in [result.kind, u32::from(result.position), result.area, result.diagnostic_status[0], result.diagnostic_status[1], result.class, result.pending, result.treated_pending, result.decision, result.fast, result.area_status].into_iter().enumerate() { word(&mut block.0, 0x108 + 4 * i, value); }
    block.0[0x134] = result.area_count;
    for (i, area) in result.areas.iter().enumerate() { half(&mut block.0, 0x138 + 8 * i, area.position as u16); half(&mut block.0, 0x13c + 8 * i, area.index); }
    block.0[0x150..0x164].copy_from_slice(&result.source);
    word(&mut block.0, 0x164, result.retry_requested);
    block
}

pub(crate) fn focus_result(bytes: &[u8; 96]) -> FocusResult {
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let half = |at: usize| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
    FocusResult {
        kind: word(0), position: half(4), area: word(8), diagnostic_status: [word(12), word(16)], class: word(20),
        pending: word(24), treated_pending: word(28), decision: word(32), fast: word(36), area_status: word(40), area_count: bytes[44],
        areas: core::array::from_fn(|i| xm1_saclac_calc::adjust::motion::plan::ResultArea { position: half(48 + 8 * i) as i16, index: half(52 + 8 * i) }),
        source: bytes[72..92].try_into().unwrap(), retry_requested: word(92),
    }
}
