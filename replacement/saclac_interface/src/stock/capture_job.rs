#![forbid(unsafe_code)]

//! X-M1 1.50 layouts of the primary job's payloads. Completion payloads start at +0x10 of the
//! receive buffer 0x169d370; the job reads the words after the status from 0x169d384.

use crate::capture_job::{AreaDelivery, CaptureEnd, CaptureOutput, CaptureRequest, CaptureStart, CaptureStatistics, CaptureWindow, DisplayRequest};
use crate::statistics::StatisticsSetting;

fn word(bytes: &[u8], offset: usize) -> Option<u32> { Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?)) }

fn half(bytes: &[u8], offset: usize) -> Option<u16> { Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?)) }

fn put(bytes: &mut [u8], offset: usize, value: &[u8]) { bytes[offset..offset + value.len()].copy_from_slice(value); }

/// 0xf2c990 posts its status, then four words; 0xf2cc20..0xf2cc28 copies them to the context
/// (+8..+0x14), which takes the halfwords at +0xc and +0xe and the words at +0x10 and +0x14 for
/// the default calibration (0xf2cc5c..0xf2cc80).
pub fn capture_start(payload: &[u8]) -> Option<CaptureStart> {
    Some(CaptureStart {
        profile: word(payload, 4)?,
        curve_pair: [half(payload, 8)?, half(payload, 0xa)?],
        curve_words: [word(payload, 0xc)?, word(payload, 0x10)?],
    })
}

/// 0xf2c9c4 posts its status, then three words; 0xf2d6a0..0xf2d6a8 copies them.
pub fn capture_end(payload: &[u8]) -> Option<CaptureEnd> {
    Some(CaptureEnd { readout: [word(payload, 4)?, word(payload, 8)?], detail: word(payload, 0xc)? })
}

/// 0xf2ca78 posts its status, then 88 bytes; 0xf2d1d0..0xf2d1dc copies them whole.
pub fn capture_output(payload: &[u8]) -> Option<CaptureOutput> {
    Some(CaptureOutput(payload.get(4..4 + CaptureOutput::BYTES)?.try_into().ok()?))
}

/// 0xf2ca04 posts its status and one word; 0xf2de40 reads the status.
pub fn area_delivery(payload: &[u8]) -> Option<AreaDelivery> { Some(AreaDelivery { found: word(payload, 0)? != 0 }) }

pub const CAPTURE_REQUEST_BYTES: usize = 0x98;

fn put_window(bytes: &mut [u8], offset: usize, window: &CaptureWindow) {
    put(bytes, offset, &window.left.to_le_bytes());
    put(bytes, offset + 2, &window.width.to_le_bytes());
    put(bytes, offset + 4, &window.half_height.to_le_bytes());
    put(bytes, offset + 6, &window.half_height.to_le_bytes());
}

/// The 152 bytes 0xfeac00 posts to imgdc 0x3f49 after its six callbacks, as 0xf2d8b0 fills them
/// (0xf2d8bc clears the block first).
pub fn capture_request(request: &CaptureRequest) -> [u8; CAPTURE_REQUEST_BYTES] {
    let mut bytes = [0; CAPTURE_REQUEST_BYTES];
    put(&mut bytes, 0x00, &request.source.to_le_bytes());
    put_window(&mut bytes, 0x04, &request.windows[0]);
    put(&mut bytes, 0x0c, &request.source_words[0].to_le_bytes());
    put(&mut bytes, 0x10, &request.source_words[1].to_le_bytes());
    put_window(&mut bytes, 0x14, &request.windows[1]);
    put(&mut bytes, 0x1c, &request.source_words[2].to_le_bytes());
    let transfer = &request.transfer;
    put(&mut bytes, 0x20, &transfer.reference.to_le_bytes());
    put(&mut bytes, 0x24, &transfer.second_limit.to_le_bytes());
    put(&mut bytes, 0x26, &transfer.trim.to_le_bytes());
    put(&mut bytes, 0x28, &transfer.offset.to_le_bytes());
    put(&mut bytes, 0x2a, &transfer.residual.to_le_bytes());
    put(&mut bytes, 0x2c, &transfer.step.to_le_bytes());
    put(&mut bytes, 0x34, &request.frame_words[0].to_le_bytes());
    put(&mut bytes, 0x38, &request.frame_words[1].to_le_bytes());
    put(&mut bytes, 0x3c, &request.layout[0].to_le_bytes());
    put(&mut bytes, 0x3e, &request.layout[1].to_le_bytes());
    put(&mut bytes, 0x40, &request.subject_source.to_le_bytes());
    put(&mut bytes, 0x44, &request.resource_kind.to_le_bytes());
    put(&mut bytes, 0x48, &request.subjects_locked.to_le_bytes());
    put(&mut bytes, 0x4c, &request.slot_reference.to_le_bytes());
    put(&mut bytes, 0x50, &request.profile_selector.to_le_bytes());
    put(&mut bytes, 0x54, &request.source_option.to_le_bytes());
    put(&mut bytes, 0x58, &request.secondary.measured.to_le_bytes());
    put(&mut bytes, 0x5c, &request.secondary.primary.to_le_bytes());
    put(&mut bytes, 0x60, &request.secondary.primary_limit.to_le_bytes());
    put(&mut bytes, 0x64, &request.options.to_le_bytes());
    put(&mut bytes, 0x68, &100u16.to_le_bytes());
    put(&mut bytes, 0x6a, &16u16.to_le_bytes());
    put(&mut bytes, 0x6c, &request.profile_class.to_le_bytes());
    for (offset, value) in [
        (0x6e, request.at_0186),
        (0x70, request.at_0184),
        (0x72, request.slot_offset),
        (0x74, request.slot_second_limit),
        (0x76, request.slot_trim),
        (0x78, request.at_017a),
        (0x7a, request.slot_residual),
        (0x7c, request.slot_step),
    ] {
        put(&mut bytes, offset, &value.to_le_bytes());
    }
    for (offset, value) in [
        (0x7e, request.secondary_mode),
        (0x7f, request.at_0036),
        (0x80, request.at_0208),
        (0x81, request.at_0039),
        (0x82, request.at_004d),
        (0x83, request.at_0053),
        (0x84, request.secondary_latch),
        (0x85, request.secondary_blocked),
        (0x86, request.at_0042),
        (0x87, u8::from(request.held)),
        (0x88, u8::from(request.secondary_pending)),
        (0x89, request.at_0188),
        (0x8a, request.source_byte),
        (0x8b, request.mode),
        (0x8c, request.at_0198),
        (0x8d, u8::from(request.paired)),
        (0x8e, request.selection_byte),
        (0x90, u8::from(request.second_limit_unset)),
        (0x91, 1),
        (0x92, request.at_006c),
        (0x93, u8::from(request.secondary_enabled)),
        (0x94, request.requested_slot),
        (0x95, request.slot_positions),
        (0x96, request.at_003e),
        (0x97, request.subject_area),
    ] {
        bytes[offset] = value;
    }
    bytes
}

pub const DISPLAY_REQUEST_BYTES: usize = 20;

/// The 20 bytes 0xfead28 posts to imgdc 0x3f4e, as 0xf2d7f0..0xf2d85c fills them.
pub fn display_request(request: &DisplayRequest) -> [u8; DISPLAY_REQUEST_BYTES] {
    let mut bytes = [0; DISPLAY_REQUEST_BYTES];
    let words = [request.setting_word, request.setting_pair[0], request.setting_pair[1], request.extent[0], request.extent[1]];
    for (index, value) in words.iter().enumerate() { put(&mut bytes, 4 * index, &value.to_le_bytes()); }
    bytes
}

/// Frame width and height of every capture's statistics (0xf2cdb0..0xf2cdbc).
const STATISTICS_FRAME: [u32; 2] = [240, 640];
/// Frame part +0x00..+0x38, window part +0x38, lens part +0x48, profile part +0x58, as
/// 0x1039b48 and the template copy 0xf2cf8c..0xf2cfcc lay them. 0xf2cc00 leaves frame bytes
/// +0x1d..+0x1f and profile byte +0x37 unwritten; they are zero here.
pub fn statistics_setting(statistics: &CaptureStatistics) -> StatisticsSetting {
    let mut bytes = [0; StatisticsSetting::BYTES];
    let frame = &statistics.frame;
    put(&mut bytes, 0x00, &frame.record_word.to_le_bytes());
    put(&mut bytes, 0x04, &frame.frame_words[0].to_le_bytes());
    put(&mut bytes, 0x08, &frame.frame_words[1].to_le_bytes());
    put(&mut bytes, 0x0c, &STATISTICS_FRAME[0].to_le_bytes());
    put(&mut bytes, 0x10, &STATISTICS_FRAME[1].to_le_bytes());
    put(&mut bytes, 0x14, &frame.layout[0].to_le_bytes());
    put(&mut bytes, 0x16, &frame.layout[1].to_le_bytes());
    put(&mut bytes, 0x18, &u32::from(frame.flagged).to_le_bytes());
    bytes[0x1c] = frame.at_0198;
    put(&mut bytes, 0x20, &frame.selector.to_le_bytes());
    for (index, value) in statistics.window.iter().enumerate() { put(&mut bytes, 0x38 + 4 * index, &value.to_le_bytes()); }
    for (index, value) in statistics.lens.iter().enumerate() { put(&mut bytes, 0x48 + 4 * index, &value.to_le_bytes()); }
    let profile = &statistics.profile;
    for (index, value) in profile.record_words.iter().enumerate() { put(&mut bytes, 0x58 + 4 * index, &value.to_le_bytes()); }
    put(&mut bytes, 0x78, &profile.calibration_word.to_le_bytes());
    for (index, value) in profile.calibration.iter().enumerate() { put(&mut bytes, 0x7c + 2 * index, &value.to_le_bytes()); }
    put(&mut bytes, 0x82, &profile.trim.to_le_bytes());
    put(&mut bytes, 0x84, &profile.second_limit.to_le_bytes());
    put(&mut bytes, 0x86, &100u16.to_le_bytes());
    put(&mut bytes, 0x88, &profile.option_word.to_le_bytes());
    bytes[0x8c] = profile.option;
    bytes[0x8d] = profile.at_003e;
    bytes[0x8e] = profile.record_byte;
    StatisticsSetting::from_bytes(bytes)
}
