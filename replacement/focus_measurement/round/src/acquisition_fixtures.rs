use crate::acquisition::ports::*;
use crate::auxiliary::ResourceIndex;
use crate::fakes::Ports;
use crate::pipeline::{GainStep, PipelineSnapshot, PipelineWord, SnapshotStatus, TimingWord};
use crate::profile::{ProfileSelection, RetainedWord};
use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_interface::round_acquisition::*;

// Synthetic profile, pipeline and calc results; no camera or captured firmware data.
pub fn configure<X>(ports: &mut Ports<X>) {
    ports.acquisition.view = RecordView {
        start: Start::Fresh, selection: ProfileSelection::Record, mode: AcquisitionMode(46), s1_mode: AcquisitionMode(47),
        halfword: 11, mode_word: 5, byte: 6, second_halfword: 7, setting_word: 8, grid_kind: 9, focus_word: 10, window: 12,
        third_word: RetainedWord(13), zero_word: RetainedWord(14), second_word: RetainedWord(15),
        fourth_word: RetainedWord(16), fifth_word: RetainedWord(17), seventh_word: RetainedWord(18),
        eighth_word: RetainedWord(19), ninth_word: RetainedWord(20), tenth_word: RetainedWord(21),
        request: 22, word: 2, reference: 100, selection_word: 24, lens_halfword: 25, lens_reference: 26,
        halfwords: [27, 28], category: 3, pair: [29, 30], change_category: 31, signed_word: 32,
        mode_flag: 1, rate_enabled: 1, profile_flags: [0, 2, 3, 4, 5, 6, 7, 8], lens_word_index: 33,
        adjustment: 34, kind: 35, kind_override: 36, pair_word: 37, program_word: [38, 39],
        restore_position: 40, s1_byte: 41, s1_halfword: 42, release_halfword: 43,
        area: [0x0002_0001, 3, 4, 5, 6], updated_position: 44, position_halfword: 45,
        converted_level: 46, restore_adjustment: 47, decade: 200, profile_index: 48, ratio: [49, 50],
        lens_limit: 51, block_reference: 52, block_word: 53, s1_exposure: PipelineWord(54), s1_level: 300, s1_second: 60,
        ..RecordView::default()
    };
    ports.snapshot = PipelineSnapshot {
        status: SnapshotStatus(5), timing: TimingWord(0x8080), exposure: [PipelineWord(0x1010), PipelineWord(0x2020)],
        levels: [800, 200], gain_step: GainStep(2), offset_level: 30, adjustment: PipelineWord(1),
        code: 0x44, flags: 2, reference: 55, extent: [PipelineWord(1), PipelineWord(2)],
        size: [PipelineWord(3), PipelineWord(4)], origin: [PipelineWord(5), PipelineWord(6)], ..PipelineSnapshot::EMPTY
    };
    ports.acquisition.gain_step_conversion = |offset| GainStep(match offset { 30 => 3, 46 => 4, other => other as u32 });
    ports.gain_offsets[2] = 20;
    ports.gain_offsets[3] = 30;
    ports.exposure_value = 40;
    ports.relative_exposure = 60;
    ports.timing_offset = 7;
    ports.acquisition.lens = ResourceIndex(0x4040);
    ports.acquisition.target = ProgramTarget { base: 600, level: 500, exposure: PipelineWord(0x3030), gain_level: 30, second: 40, offset: 50, ..ProgramTarget::default() };
    ports.acquisition.range = Range { timing: TimingWord(0x9090), level: 700, first: 100, second: 20, code: 0x66, selected: 3 };
    ports.acquisition.configuration = 0x71;
    ports.acquisition.category = 0x72;
    ports.acquisition.grid_count = 2;
    ports.acquisition.limits = SelectionLimits { step: 61, lower: 62, upper: 63, byte: 64 };
    ports.acquisition.positions[2] = [0x5050, 0x6060];
    ports.acquisition.saved = SavedExposure { exposure: 0x7070, level: 70, second: 71, aspect: 72, offset: 73 };
    ports.acquisition.echo = SettingEcho { ready: 1, updated_position: 0x81, position_halfword: 0x82, shot_word: 0x83 };
}

pub fn primary_request(branch: Branch, first: bool) -> AcquisitionRequest {
    let s1 = branch == Branch::S1;
    AcquisitionRequest {
        mode: if s1 { 47 } else { 46 }, first,
        profile: AcquisitionProfile { halfword: if s1 { 0 } else { 11 }, updated_position: 44, word: 2, reference: 100,
            selection: 1, adjustment: 34, area: [0x0002_0001, 3, 4, 5, 6], category: 3, ratio: [49, 50], window: 12 },
        sample: AcquisitionSample { complete: true, exposure: 0x1010, level: 800, reference_level: if s1 { 170 } else { 180 },
            gain_step: if s1 { 3 } else { 2 }, offset_level: 30, adjustment: if s1 { 0 } else { 1 }, timing: 0x8080,
            halfword: 55, extent: [1, 2], size: [3, 4], origin: [5, 6] },
        relative_level: if s1 { 440 } else { 330 }, configuration: 0x71, level_category: 0x72,
        correction: 0, aspect: 7, decade: 200,
    }
}

pub fn selection_request(branch: Branch, first: bool, recording: bool) -> SelectionRequest {
    let s1 = branch == Branch::S1;
    SelectionRequest {
        first, mode: if s1 { 47 } else { 46 }, halfword: 11, third_word: RetainedWord(13), zero_word: RetainedWord(14), mode_word: 5,
        byte: 6, second_halfword: 7, lens: if s1 { 0x5050 } else { 0x4040 }, converted_step: 4, converted_level: 46,
        reference: 100, restore_adjustment: 47, selection_word: 24, selection: 1, exposure: if s1 { 0x1010 } else { 0x1010 },
        summary_level: if s1 { 310 } else { 600 }, limits: SelectionLimits { step: 61, lower: 62, upper: 63, byte: 64 },
        lens_halfword: 25, lens_reference: 26, limit_halfwords: [27, 28], gain_step: if s1 { 0 } else { 2 },
        unrestored: false, correction: 0, aspect: 7, decade: 200, adjustment_class: 0, eighth_word: RetainedWord(19), grid_count: 2,
        second_word: RetainedWord(15), adjustment: 34, signed_word: 32, seventh_word: RetainedWord(if s1 { 0 } else { 18 }),
        fourth_word: RetainedWord(16), fifth_word: RetainedWord(17), focus_word: 10, word: 2, rate_mode: 0, resource_level: 60,
        mode_flag: 1, kind: 35, ninth_word: RetainedWord(20), option: 0, area: [1, 2, 3], pair_word: 37, program_word: 38,
        rate_flag: false, program: 35, request: 22, eligible: u32::from(s1), window: s1 || recording, shot_copy: s1 || recording,
        scale_change: false, program_changed: 0, program_flag: 8, offset_mode: 0,
        s1: if s1 { SelectionS1 { halfword: 42, margin: 65488, byte: 41, position: 0x5050 } } else { SelectionS1::default() },
        recording: s1 || recording,
    }
}

pub fn primary_result() -> AcquisitionResult {
    AcquisitionResult { address: SharedAddress(0x1690000), levels: PrimaryLevels { base: 900, levels: [901, 902, 903, 904, 905, 906, 907], word: 0xa1 },
        statistics: [SharedAddress(0x1000), SharedAddress(0x2000), SharedAddress(0x3000), SharedAddress(0x4000)],
        exposure: 0x5050, level: 1000, second: 100, offset: 20, aspect: 7, halfword: 9, gain_offset: 30, correction: 2, count: 3, statistics_valid: true }
}

pub fn selection_result() -> SelectionResult {
    SelectionResult { address: SharedAddress(0x1691000), word: 3,
        levels: SelectionLevels { base: 101, levels: [102, 103, 104], zeroed: [105, 106, 107, 108, 109, 110], unsigned: 111,
            word: 112, last: 113, reference: 114, tail: [115, 116, 117] },
        primary: SelectionProgram { exposure: 0x8000, levels: [201, 202, 203, 204], first: [205, 206, 207, 208], second: [209, 210, 211, 212], adjustment: [213, 214, 215, 216] },
        secondary: SelectionProgram { exposure: 0x9000, levels: [301, 302, 303, 304], first: [305, 306, 307, 308], second: [309, 310, 311, 312], adjustment: [313, 314, 315, 316] },
        restore_levels: [118, 119], position: 0x8181, halfwords: [120, 121], words: [2, 1, 1], table_reference: 122,
        table_word: 123, primary_only: 0, program_argument: 124, alternate: 125, kind: 126,
        shot_record: SharedAddress(0x1692000), saved_record: SharedAddress(0x1693000), selected: 1,
        shot_data: true, keeps_program: false, adjustment: 127, program_word: 128, limit: 129, program_flag: 130,
        tail: [131, 132, 133, 134], level_halfword: 135, position_word: 0x8282, position_flag: 1,
        save: SavedSelection { index: 136, word: 137, bytes: [138, 139], level: -140, flag: 141 }, }
}
