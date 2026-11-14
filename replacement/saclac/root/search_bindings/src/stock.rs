#![forbid(unsafe_code)]

//! X-M1 1.50 retained entries and layouts used by the search bindings.
//! Decomp revision 55436fc85c0560dd252b0410af6f2427fe198cd3; argument widths checked in id4/id5.

use xm1_saclac_focus_search::contrast::{FocusLog, FocusLogEntry, StageRecord};
use xm1_saclac_shared::settings::{AdjustmentKey, Adjustments};
use xm1_saclac_shared::stock::{Argument, Machine};

#[derive(Clone, Copy)]
pub struct Entry(u32);
macro_rules! entries { ($($name:ident = $value:expr;)*) => { $(pub const $name: Entry = Entry($value);)* }; }
entries! {
    ADJUSTMENT = 0x69f8fc; LENS_POSITION = 0x76da84; LENS_IDENTITY = 0x76d9c8;
    LENS_FLAGS = 0x76f340; LENS_REFERENCE = 0x76dbb4; LENS_STATE = 0x746054;
    LENS_MODE = 0x76f360; LENS_BUSY = 0x76da38; LENS_READY = 0x76b8b4;
    LENS_MOVING = 0x76d9a8; LENS_MOVE = 0x76d9ec;
    OUTER_LIMITS = 0x74bd64; LENS_TABLE = 0x7462e8; LENS_LIMITS = 0x74b05c;
    CHANNEL_CURVE = 0x748818; SELECTION_WORD = 0x748398; REMAP = 0x74947c;
    EXPORT_WORD = 0x7498e0; ADJUST_DIRECTION = 0x747aac; SCALED_LIMIT = 0x749254;
    TABLE_SCALE = 0x74888c; CURVE_POSITION = 0x746120; CURVE_VALUE = 0x7461a8;
    CHANNEL_WORD = 0x747408; REFERENCE_WORD = 0x747584; APPLY_REFERENCE = 0x749be4;
    REFERENCE_SCALE = 0x1007e44; FRAME_INTERVAL = 0x1005f60;
    READOUT_START = 0x1004d04; REFERENCE_LINE = 0x1004ce0; LINE_PERIOD = 0x1004d68;
    LINEAR = 0xee9ab0; LOG_DIFFERENCE = 0xee99f0; LOGARITHM = 0xee97f0;
    CONTRAST_LOG = 0xee9928; SNAPSHOT = 0xfea7ac; PROFILE_CLASS = 0xfea7bc;
    S1_STAGE = 0xfeaacc; S1_SNAPSHOT = 0xfeaadc;
    SELECTION_RATIO = 0xf5206c; CONVERTED_DECADE = 0xef74e4; ZONE_LIMIT = 0x772f28;
    LENS_STEP = 0xf77ef8; PROGRAM_LEVEL_WORD = 0xf784f8;
    SELECT_PROGRAM = 0xf77c8c; PROGRAM_LEVEL = 0xf78d24;
    FAULT = 0xf46c20; RESET_LOG = 0xf47468; TICKS = 0x7257a8;
    ANGULAR_RATE = 0x6b7830; REQUEST_ANGULAR_RATE = 0x6b7840;
    DEBUG_TEXT = 0x1110918; TRACE = 0x64744c;
}

pub const LIVE_EVALUATED: u32 = 0xf3c104;
pub const S1_EVALUATED: u32 = 0xf25758;
pub const LIVE_REPORTED: u32 = 0xf3c14c;
pub const S1_REPORTED: u32 = 0xf25728;
const LOG_BASE_WORD: u32 = 0xf49358;
const LOG_INDEX: u32 = 0x16c2228;
const SHOT_POSITION: u32 = 0x16cb404;
const TRACE_TAG: u32 = 0xffb104;
const TRACE_VALUE: u32 = 0xf46abc;
const TRACE_NEGATIVE: u32 = 0xf46ac0;

pub fn call(machine: &impl Machine, entry: Entry, values: &[u32]) -> u32 {
    let mut args = [0usize; 17];
    for (out, &value) in args.iter_mut().zip(values) {
        *out = value as usize;
    }
    machine.enter(entry.0, &args[..values.len()])
}
pub fn buffer(machine: &impl Machine, entry: Entry, args: &[Argument], bytes: &mut [u8]) -> u32 {
    machine.enter_with(entry.0, args, bytes)
}
pub const fn w(value: u32) -> Argument {
    Argument::Word(value)
}
pub const fn out(offset: usize) -> Argument {
    Argument::Buffer(offset)
}
pub const fn signed(value: i16) -> u32 {
    value as i32 as u32
}
pub fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
}
pub fn half(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap())
}
pub fn put_word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
pub fn put_half(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
pub fn read<const N: usize>(machine: &impl Machine, address: u32) -> [u8; N] {
    core::array::from_fn(|index| {
        let at = address.wrapping_add(index as u32);
        (machine.read_u16(at & !1) >> (8 * (at & 1))) as u8
    })
}
fn write(machine: &mut impl Machine, address: u32, bytes: &[u8]) {
    for (offset, &byte) in bytes.iter().enumerate() {
        machine.write_u8(address.wrapping_add(offset as u32), byte);
    }
}
pub fn report_position(machine: &mut impl Machine, position: i16) {
    write(machine, SHOT_POSITION, &position.to_le_bytes());
}

pub struct Area<'a, M>(pub &'a M);
impl<M: Machine> Adjustments for Area<'_, M> {
    fn byte(&self, key: AdjustmentKey) -> u8 {
        call(self.0, ADJUSTMENT, &[key.0]) as u8
    }
    fn lens_adjustment(&self) -> u16 {
        xm1_saclac_shared::settings::halfword(self, AdjustmentKey(0x3e0243))
    }
    fn admits_unselected_mode(&self) -> bool {
        self.byte(AdjustmentKey(0x887c)) != 0
    }
}
pub fn setting(
    area: &impl Adjustments,
    setting: xm1_saclac_focus_search::contrast::settings::Setting,
) -> u32 {
    use xm1_saclac_focus_search::stock::{setting_key, AdjustmentWidth};
    let (key, width) = setting_key(setting);
    match width {
        AdjustmentWidth::Byte => u32::from(area.byte(AdjustmentKey(key))),
        AdjustmentWidth::Half => u32::from(xm1_saclac_shared::settings::halfword(
            area,
            AdjustmentKey(key),
        )),
        AdjustmentWidth::SignedHalf => {
            xm1_saclac_shared::settings::halfword(area, AdjustmentKey(key)) as i16 as i32 as u32
        }
        AdjustmentWidth::SignedByte => area.byte(AdjustmentKey(key)) as i8 as i32 as u32,
    }
}
pub fn table_offset(
    area: &impl Adjustments,
    lens: i32,
    kind: xm1_saclac_focus_search::contrast::TableOffset,
) -> u16 {
    use xm1_saclac_focus_search::contrast::TableOffset;
    let base: u32 = match kind {
        TableOffset::Default => 0x3e07f7,
        TableOffset::First => 0x3e08c0,
        TableOffset::Second => 0x3e0989,
    };
    xm1_saclac_shared::settings::halfword(
        area,
        AdjustmentKey(base.wrapping_add(2u32.wrapping_mul(lens as u32))),
    )
}
pub fn table_count(area: &impl Adjustments) -> u32 {
    u32::from(area.byte(AdjustmentKey(0x3e1214)))
}
pub fn trace_flags(area: &impl Adjustments) -> u16 {
    xm1_saclac_shared::settings::halfword(area, AdjustmentKey(0xe8))
}
pub fn thresholds(area: &impl Adjustments) -> [i8; 2] {
    [
        area.byte(AdjustmentKey(0x8b16)) as i8,
        area.byte(AdjustmentKey(0x8b1f)) as i8,
    ]
}

pub fn stage(bytes: &[u8; 24]) -> StageRecord {
    StageRecord {
        stage: word(bytes, 0),
        first_pass: bytes[4],
        words: [word(bytes, 8), word(bytes, 16)],
        halves: [half(bytes, 12), half(bytes, 20)],
        last: bytes[22],
    }
}
pub fn area_levels(machine: &impl Machine, descriptor: &[u8; 12]) -> [u16; 2] {
    let address = word(descriptor, 8).wrapping_add(u32::from(descriptor[4]).wrapping_mul(0x228));
    [
        machine.read_u16(address.wrapping_add(0x204)),
        machine.read_u16(address.wrapping_add(0x206)),
    ]
}
pub fn pinned_status(
    bytes: &mut [u8; 76],
    status: xm1_saclac_focus_search::contrast::PinnedStatus,
) {
    put_word(bytes, 64, status.level);
    put_word(bytes, 68, status.mode);
    put_half(bytes, 72, status.position.0 as u16);
}

pub fn append_log(machine: &mut impl Machine, log: &FocusLog) {
    let base = machine.read_u32(LOG_BASE_WORD);
    let index = machine.read_u32(LOG_INDEX);
    let address = base.wrapping_add(index.wrapping_mul(108));
    let mut bytes = read::<108>(machine, address);
    encode_log(&mut bytes, &log.entry);
    write(machine, address, &bytes);
    for (entries, halves, words, positions) in [
        (&log.scan[..], 0x7efc, 0x7f0c, 0x7f28),
        (&log.extent[..], 0x7f36, 0x7f50, 0x7f84),
    ] {
        for (i, sample) in entries.iter().enumerate() {
            write(
                machine,
                base + halves + i as u32 * 2,
                &sample.focus_value.to_le_bytes(),
            );
            machine.write_u32(base + words + i as u32 * 4, sample.contrast);
            write(
                machine,
                base + positions + i as u32 * 2,
                &sample.position.to_le_bytes(),
            );
        }
    }
    for offset in (0x7fa0..0x7fb8).step_by(4) {
        machine.write_u32(base + offset, 0);
    }
    machine.write_u32(LOG_INDEX, if index >= 300 { 0 } else { index + 1 });
}
fn encode_log(bytes: &mut [u8; 108], e: &FocusLogEntry) {
    for (at, value) in [
        (0, e.phase),
        (2, e.profile_focus as u16),
        (4, e.parameter),
        (6, e.snapshot_level),
        (12, e.lens_position as u16),
        (14, e.focus_mark),
        (16, e.pinned_setting),
        (18, e.selected),
        (36, e.rising),
        (38, e.falling),
        (40, e.fit_position as u16),
        (42, e.fit),
        (88, e.ring_focus_value),
        (90, e.track_flag),
        (92, e.ring_levels[0]),
        (94, e.ring_levels[1]),
        (96, e.track_marks[0]),
        (98, e.track_marks[1]),
        (100, e.scan_flag),
        (102, e.track_index),
        (104, e.display_word),
        (106, e.track_mark),
    ] {
        put_half(bytes, at, value);
    }
    for (at, value) in [
        (8, e.level as u32),
        (20, e.combined),
        (24, e.second),
        (28, e.flag),
        (32, 0),
        (44, e.lens),
        (48, e.monitor_mark as u32),
        (52, e.movement[0]),
        (56, e.movement[1]),
        (80, e.ticks),
        (84, e.ring_contrast),
    ] {
        put_word(bytes, at, value);
    }
    if let Some(values) = e.phase_words {
        for (i, value) in values.into_iter().enumerate() {
            put_word(bytes, 60 + 4 * i, value);
        }
    }
}
pub fn newest_log(machine: &impl Machine) -> FocusLogEntry {
    let base = machine.read_u32(LOG_BASE_WORD);
    let next = machine.read_u32(LOG_INDEX);
    let bytes = read::<108>(
        machine,
        base.wrapping_add(if next == 0 { 300 } else { next - 1 }.wrapping_mul(108)),
    );
    let h = |at| half(&bytes, at);
    let w = |at| word(&bytes, at);
    FocusLogEntry {
        phase: h(0),
        profile_focus: h(2) as i16,
        parameter: h(4),
        snapshot_level: h(6),
        level: w(8) as i32,
        lens_position: h(12) as i16,
        focus_mark: h(14),
        pinned_setting: h(16),
        selected: h(18),
        combined: w(20),
        second: w(24),
        flag: w(28),
        rising: h(36),
        falling: h(38),
        fit_position: h(40) as i16,
        fit: h(42),
        lens: w(44),
        monitor_mark: w(48) as i32,
        movement: [w(52), w(56)],
        phase_words: Some([w(60), w(64), w(68), w(72), w(76)]),
        ticks: w(80),
        ring_contrast: w(84),
        ring_focus_value: h(88),
        track_flag: h(90),
        ring_levels: [h(92), h(94)],
        track_marks: [h(96), h(98)],
        scan_flag: h(100),
        track_index: h(102),
        display_word: h(104),
        track_mark: h(106),
    }
}

pub fn debug_text(machine: &impl Machine, text: &[u8], column: u8, row: u8) {
    let mut bytes = [0; 256];
    let count = text
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(text.len())
        .min(bytes.len() - 1);
    bytes[..count].copy_from_slice(&text[..count]);
    buffer(
        machine,
        DEBUG_TEXT,
        &[
            out(0),
            w(u32::from(column)),
            w(u32::from(row)),
            w(0xff),
            w(7),
        ],
        &mut bytes,
    );
}
pub fn trace_values(machine: &impl Machine, values: &[i32]) {
    for &value in values {
        call(
            machine,
            TRACE,
            &[
                TRACE_TAG,
                if value < 0 {
                    TRACE_NEGATIVE
                } else {
                    TRACE_VALUE
                },
                value.unsigned_abs(),
            ],
        );
    }
}
