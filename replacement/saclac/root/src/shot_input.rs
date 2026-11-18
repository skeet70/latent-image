use xm1_saclac_shared::stock::Machine;

use xm1_saclac_interface::exposure::CalculationInput;

fn pair(low: u16, high: u16) -> u32 { u32::from(low) | (u32::from(high) << 16) }

// Publication of a newly built input into the one machine-backed buffer calc reads.
macro_rules! words {
    ($operation:ident, $input:ident, $offset:ident, $value:ident; $($at:literal => $field:ident),* $(,)?) => {
        match $offset { $($at => words!(@$operation $input, $field, $value),)* _ => None }
    };
    (@get $input:ident, $field:ident, $value:ident) => { Some($input.$field) };
}

macro_rules! scalar_words {
    ($operation:ident, $input:ident, $offset:ident, $value:ident) => {
        words!($operation, $input, $offset, $value;
            0x08 => lens_reference, 0x0c => target_word,
            0x24 => level_profile, 0x2c => level_policy, 0x40 => plan_kind,
            0x50 => measurement_selection, 0x54 => selection_kind, 0x5c => record_32,
            0x64 => record_5b, 0x68 => record_5d, 0x6c => record_34,
            0x70 => record_3d, 0x74 => record_3e, 0x78 => selection_style,
            0x7c => record_43, 0x84 => record_14, 0x8c => source,
            0x98 => record_45, 0x9c => selection_auxiliary,
            0xa0 => header_47, 0xa4 => header_61, 0xa8 => record_4b,
            0xac => header_63, 0xb0 => interval_tracking, 0xb4 => record_51,
            0xb8 => record_66, 0xbc => record_67, 0xc0 => record_68,
            0xc4 => record_69, 0xc8 => record_20c8, 0xcc => record_20cc,
            0xd0 => record_2110, 0xd4 => record_20d4, 0xd8 => record_20b0,
            0xdc => record_5f, 0xe0 => capture_offset, 0xe4 => offered_measurements,
            0xe8 => window_status, 0xec => shared_738, 0xf0 => second_result,
            0xf4 => grid_layout, 0xf8 => shared_85c,
            0x100 => record_lens_reference, 0x11c => option)
    };
}

fn word(input: &CalculationInput, offset: usize) -> u32 {
    if let Some(value) = scalar_words!(get, input, offset, unused) { return value; }
    match offset {
        0x04 => if input.selection { 4 } else { 3 },
        0x10 => u32::from(input.target_half),
        0x14 | 0x18 => input.area[(offset - 0x14) / 4],
        0x1c => pair(input.target_level as u16, input.start_level as u16),
        0x20 => u32::from(input.measured_level),
        0x28 => u32::from(input.unmeasured),
        0x30 | 0x128 => u32::from(input.refinement_pass),
        0x34 => u32::from(input.first_treatment),
        0x38 => u32::from(input.metering_level),
        0x3c => u32::from_le_bytes([input.grid[0], input.grid[1], input.grid_steps[0], input.grid_steps[1]]),
        0x44 | 0x48 => { let index = (offset - 0x44) / 2; pair(input.plan_exposure[index], input.plan_exposure[index + 1]) }
        0x4c => u32::from(input.plan_exposure[4]),
        0x58 => u32::from(input.selection_flags),
        0x60 => u32::from(input.record_33) | (u32::from(input.record_26) << 16),
        0x80 => u32::from(input.offset as u16),
        0x88 => u32::from(input.lead as u16),
        0x90 => pair(input.record_20 as u16, input.record_level as u16),
        0x94 => u32::from(input.record_24 as u16),
        0x104 | 0x108 | 0x10c | 0x110 => {
            let index = (offset - 0x104) / 2;
            pair(input.levels[index] as u16, input.levels[index + 1] as u16)
        }
        0x114 => u32::from(input.levels[8] as u16),
        0x120 => 4,
        _ => 0,
    }
}

pub(crate) fn store(machine: &mut impl Machine, input: &CalculationInput) {
    for offset in (0..xm1_shot_sequence::stock::calculation::INPUT_BYTES).step_by(4) {
        machine.write_u32(xm1_shot_sequence::stock::calculation::INPUT + offset as u32, word(input, offset));
    }
}
