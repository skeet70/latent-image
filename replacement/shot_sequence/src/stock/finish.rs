#![forbid(unsafe_code)]

//! X-M1 1.50 boundary values of the finish step (machine 6, 0xf17b40..0xf1a0fc). The finish
//! context 0x16a04d8 and the remembered position 0xf1a100 have no reader outside the finish and
//! are `FinishWork` fields.

use crate::boundary::LensSnapshot;
use crate::finish_work::{FinishConfiguration, FinishLensValue, FinishProfile, FinishTrace};

/// Lens-snapshot word +0x24, the capture pipeline's status test of 0xf17cd8..0xf17cf4.
const SNAPSHOT_CAPTURE_STATUS: usize = 0x24 - 36;

pub fn with_capture_status(mut snapshot: LensSnapshot, active: bool) -> LensSnapshot {
    snapshot.tail[SNAPSHOT_CAPTURE_STATUS..SNAPSHOT_CAPTURE_STATUS + 4].copy_from_slice(&u32::from(active).to_le_bytes());
    snapshot
}

/// Word +0x14 of the capture pipeline's operation state 0x1706a20, read by 0x10538a4.
pub const CAPTURE_STATUS: u32 = 0x170_6a20 + 0x14;

/// Image-4 accessors behind each profile value; `LensControl` is 0x752a40 with a second argument 0.
pub const fn profile_accessor(value: FinishProfile) -> u32 {
    match value {
        FinishProfile::Override => 0x74fa2c,
        FinishProfile::SelectionClass => 0x74e5c0,
        FinishProfile::LensMode => 0x7509c4,
        FinishProfile::LensKind => 0x754d9c,
        FinishProfile::AreaFlag => 0x75093c,
        FinishProfile::LensControl => 0x752a40,
        FinishProfile::LiveTarget => 0x757b78,
        FinishProfile::DefaultTarget => 0x756fe4,
    }
}

pub const fn lens_accessor(value: FinishLensValue) -> u32 {
    match value {
        FinishLensValue::Position => 0x76da84,
        FinishLensValue::PrefersRelative => 0x76dac4,
        FinishLensValue::Drive => 0x76f340,
        FinishLensValue::Direction => 0x76dad4,
        FinishLensValue::Distance => 0x76d9c8,
        FinishLensValue::Step => 0x76fc4c,
        FinishLensValue::MotorState => 0x76da38,
        FinishLensValue::FocusState => 0x76d9a8,
        FinishLensValue::Busy => 0x744ae8,
        FinishLensValue::Manual => 0x746018,
        FinishLensValue::ReferenceLimit => 0x747b50,
    }
}

/// The lens pair 0x76fbac writes (a word, then a halfword).
pub const LENS_PAIR: u32 = 0x76fbac;

/// Configuration entries: bytes through 0x69f8fc, `PositionUnit` a halfword through 0x69fec8.
pub const fn configuration_entry(value: FinishConfiguration) -> u32 {
    match value {
        FinishConfiguration::KeepPoint => 0x8bad,
        FinishConfiguration::PositionUnit => 0x3e_1770,
        FinishConfiguration::PositionScale => 0x8bb0,
        FinishConfiguration::DriveControl => 0xa8,
        FinishConfiguration::DirectionClamp => 0x3e_0246,
    }
}

/// The load-test setting 0x742064.
pub const LOAD_TEST: u32 = 0x742064;

/// Image-4 record and lens-layer entries the finish calls: designated point 0x752110 (record,
/// point), area positions 0x758940 / 0x758980 (36 bytes), the correction record 0x756940 /
/// 0x756970 (+0 enabled, +0x18 pending), step clamp 0x747aac, adjustment 0x74947c, bounds 0x749e98
/// and 0x749fac, lens target 0x76fb00, per-area selection 0x754148 and mode 0x754098.
pub const DESIGNATED_POINT: u32 = 0x752110;
pub const AREA_POSITIONS: u32 = 0x758940;
pub const STORE_AREA_POSITIONS: u32 = 0x758980;
pub const CORRECTION_RECORD: u32 = 0x756940;
pub const STORE_CORRECTION_RECORD: u32 = 0x756970;
pub const CORRECTION_ENABLED: usize = 0;
pub const CORRECTION_PENDING: usize = 0x18;
pub const CLAMP_STEP: u32 = 0x747aac;
pub const ADJUST: u32 = 0x74947c;
pub const NEAR_BOUNDS: u32 = 0x749e98;
pub const FAR_BOUNDS: u32 = 0x749fac;
pub const LENS_TARGET: u32 = 0x76fb00;
pub const AREA_SELECTION: u32 = 0x754148;
pub const AREA_MODE: u32 = 0x754098;

/// Area positions: the last index byte at +0, then 8-byte entries from +4 whose first halfword is
/// the position (0xf181c0..0xf18250).
pub fn area_positions(bytes: &[u8; 36]) -> crate::finish_work::AreaPositions {
    let mut positions = [0; 4];
    for (index, position) in positions.iter_mut().enumerate() {
        let offset = 4 + 8 * index;
        *position = i16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    }
    crate::finish_work::AreaPositions { last: bytes[0], positions, bytes: *bytes }
}

pub fn area_positions_bytes(positions: &crate::finish_work::AreaPositions) -> [u8; 36] {
    let mut bytes = positions.bytes;
    for (index, position) in positions.positions.iter().enumerate() {
        let offset = 4 + 8 * index;
        bytes[offset..offset + 2].copy_from_slice(&position.to_le_bytes());
    }
    bytes
}

/// Machine 3's target-source flags: bits 1, 2 and 3 of the byte at 0x169d508 (0xf153a0, 0xf15414,
/// 0xf154b0).
pub const TARGET_SOURCE: u32 = 0x169_d508;

/// The capture command block 0x169d09c: +0x38 names the count and limit bytes, +0x14 the target
/// list whose entries hold the target at +4 (0xf17dec); 0xf2a200 clamps the count.
pub const CAPTURE_COMMAND: u32 = 0x169_d09c;

/// Frame words: the applied correction at +0x55c0c and the drive word at +0x56278 (0xf18298,
/// 0xf182f8).
pub const FRAME_CORRECTION: u32 = 0x5_5c0c;
pub const FRAME_DRIVE: u32 = 0x5_6278;

/// Calculation-block and saved-input words 0xf188c8 uses: the area count at +0x5c, the selection
/// at +0x6410 and the selection mode at +0x6414 of 0x16ad740, exposure word +0x126c (1 holds the
/// saved mode), and the saved selection mode at +4 of 0x41717a54.
pub const CALCULATION_AREAS: u32 = 0x5c;
pub const SAVED_SELECTION_MODE: u32 = 0x4171_7a54 + 4;

/// Calc-library entries: point selection 0xf4e620, target selection 0xf60efc, mode selection
/// 0xf616e8, correction 0xf4c930; the shared position adjustment 0xf47300 and finish record load
/// 0xf19634.
pub const POINT_SELECTION: u32 = 0xf4e620;
pub const TARGET_SELECTION: u32 = 0xf60efc;
pub const MODE_SELECTION: u32 = 0xf616e8;
pub const CORRECTION: u32 = 0xf4c930;
pub const ADJUST_POSITION: u32 = 0xf47300;
pub const LOAD_FINISH_RECORD: u32 = 0xf19634;

/// The client lens-bounds notification 0xefb9f0 (bounds, 0).
pub const NOTIFY_BOUNDS: u32 = 0xefb9f0;

/// Words passed to the trace entry 0x72872c.
pub const fn trace_word(trace: FinishTrace) -> u32 {
    match trace {
        FinishTrace::RelativeMove => 0x0204_0000,
        FinishTrace::AbsoluteMove => 0x0204_0100,
        FinishTrace::RelativeDone => 0x1204_0000,
        FinishTrace::AbsoluteDone => 0x1204_0100,
    }
}
