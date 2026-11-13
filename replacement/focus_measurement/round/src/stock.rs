#![forbid(unsafe_code)]

use crate::lanes::{Lane, Lanes, Selection};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecordSelection(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SearchModeFlags(pub u32);

/// The profile byte 0x74e5c0 returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileSelectionByte(pub u8);

/// X-M1 1.50: 0xf32e88..0xf32ef8 and 0xf32f80..0xf33008.
pub const fn selection(word: RecordSelection) -> Selection {
    Selection {
        acquisition: word.0 & 1 != 0,
        search: word.0 & 2 != 0,
        auxiliary: word.0 & 4 != 0,
        image: word.0 & 8 != 0,
        source: word.0 & 32 != 0,
    }
}

/// `None` when the whole word is zero, which drains bookkeeping at 0xf3305c..0xf3306c.
pub const fn availability(word: RecordSelection) -> Option<Selection> {
    if word.0 == 0 { None } else { Some(selection(word)) }
}

/// X-M1 1.50: 0xf32f60..0xf32f7c gates search and image with property 0x3e0009.
pub const fn search_allowed(property: u32) -> bool { property != 1 }

/// X-M1 1.50: 0xf330e4..0xf33138 tests bits 1, 2 and 3 of property 0x88ef in that order.
pub const fn alternate_search_rule(flags: SearchModeFlags) -> crate::record::AlternateSearch {
    use crate::record::AlternateSearch;
    if flags.0 & 2 != 0 { AlternateSearch::WithRecordSelection }
    else if flags.0 & 4 != 0 { AlternateSearch::WithoutSelection }
    else if flags.0 & 8 != 0 { AlternateSearch::Always }
    else { AlternateSearch::Never }
}

/// X-M1 1.50: profile byte 0x30 (0x74e5c0 → 0x75b054); its readers in the round
/// compare it with 0 and 1 only.
pub const fn profile_selection(byte: ProfileSelectionByte) -> crate::profile::ProfileSelection {
    use crate::profile::ProfileSelection;
    match byte.0 { 0 => ProfileSelection::Unselected, 1 => ProfileSelection::Record, other => ProfileSelection::Other(other) }
}

/// X-M1 1.50: 0xf04840..0xf0499c and 0xf32530..0xf325cc use this lane encoding.
pub fn lane_flags(lanes: Lanes) -> u8 {
    [(Lane::Acquisition, 1), (Lane::Search, 2), (Lane::Auxiliary, 4), (Lane::Image, 8),
     (Lane::Window, 32), (Lane::Source, 64), (Lane::Bookkeeping, 16)]
        .into_iter().fold(0, |flags, (lane, bit)| if lanes.contains(lane) { flags | bit } else { flags })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SchedulingProperty(u32);
impl SchedulingProperty { pub const fn word(self) -> u32 { self.0 } }

/// X-M1 1.50: 0xfa72d0..0xfa72e8 reads the configuration bytes keyed at 0xfab110
/// (seven per work, by mode word); mode words 2 and 6 are never stored at 0x16d877c.
pub const fn cadence_property(work: crate::planner::Work, mode: crate::planner::Mode) -> Option<SchedulingProperty> {
    use crate::planner::{Mode, Work};
    let base = match work {
        Work::PrimaryAcquisition => 0x4bc0,
        Work::Search => 0x4bc6,
        Work::Image => 0x4bcc,
        Work::Auxiliary | Work::Source => 0x4bd2,
        Work::SecondaryAcquisition | Work::Window => return None,
    };
    let offset = match mode {
        Mode::Default => 5, Mode::RecordSelected => 0,
        Mode::ProfileSelected => 2, Mode::AboveThreshold => 3, Mode::BelowThreshold => 4,
    };
    Some(SchedulingProperty(base + offset))
}

/// X-M1 1.50: 0xfa9a20 reads this configuration byte as the frame-rate threshold.
pub const FRAME_RATE_THRESHOLD: SchedulingProperty = SchedulingProperty(0x4bd8);

pub const fn cadence(word: u32) -> crate::planner::Cadence {
    crate::planner::Cadence { first: word & 1 != 0, second: word & 2 != 0, third: word & 4 != 0, fourth: word & 8 != 0 }
}

fn word(parameters: xm1_saclac_interface::Parameters<'_>, offset: usize) -> u32 {
    parameters.bytes().get(offset..offset + 4).map_or(0, |bytes| u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// A parameter change (command 0x17).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ParameterChange {
    pub client: crate::round::ParameterClient,
    pub requested: Selection,
    pub offered: Selection,
}

/// X-M1 1.50: 0xf3300c reads words 1 and 2 of the 0x17 parameter block (0xef9dc0
/// returns 0x169d22c); 0xefb4b8 enters word 0 with the status.
pub fn parameter_change(parameters: xm1_saclac_interface::Parameters<'_>) -> ParameterChange {
    ParameterChange {
        client: crate::round::ParameterClient(xm1_saclac_interface::stock::optional_callback(word(parameters, 0))),
        requested: selection(RecordSelection(word(parameters, 4))),
        offered: selection(RecordSelection(word(parameters, 8))),
    }
}

/// X-M1 1.50: the image row function caches the channel from word 3 of command
/// 0x14 (0xefe2fc) and word 2 of command 0x2b (0xefe3ec); the replies at
/// 0xefb328 and 0xefb438 take the callback from word 0 and the value from word 4
/// of 0x14 and word 3 of 0x2b.
pub fn image_client(kind: crate::image::ImageRequest, parameters: xm1_saclac_interface::Parameters<'_>) -> crate::image::ImageClient {
    use crate::image::{ImageChannel, ImageClient, ImageRequest, ReplyValue};
    let (channel, value) = match kind { ImageRequest::Image => (12, 16), ImageRequest::Snapshot => (8, 12) };
    ImageClient {
        callback: xm1_saclac_interface::stock::optional_callback(word(parameters, 0)),
        channel: ImageChannel(word(parameters, channel)),
        value: ReplyValue(word(parameters, value)),
    }
}

/// X-M1 1.50: 0xefb3a8 and 0xefb3f0 enter word 0 of command 0x15 or 0x16.
pub fn interruption_callback(parameters: xm1_saclac_interface::Parameters<'_>) -> Option<xm1_saclac_interface::ClientCallback> {
    xm1_saclac_interface::stock::optional_callback(word(parameters, 0))
}

/// X-M1 1.50: 0xefb37c..0xefb38c and 0xefb48c..0xefb49c replace the value of a
/// reply whose channel 0x74a214 rejects with 0x74a740(128, 0).
pub const NO_CHANNEL_VALUE: crate::image::ReplyValue = crate::image::ReplyValue(128 << 16);

/// X-M1 1.50: 0xf3a160..0xf3a1b0 and 0xf3a1b8..0xf3a208 pass the channel, the
/// value and the lens bits (2 for 0x76d9a8, 1 for 0x76da38, 4 for 0x76dba4) as
/// the callback's arguments after the status.
pub fn image_reply_values(channel: crate::image::ImageChannel, value: crate::image::ReplyValue, lens: crate::image::LensReport) -> [u8; 12] {
    let bits = u32::from(lens.first) * 2 + u32::from(lens.second) + u32::from(lens.third) * 4;
    let mut values = [0; 12];
    for (slot, word) in values.chunks_exact_mut(4).zip([channel.0, value.0, bits]) {
        slot.copy_from_slice(&word.to_le_bytes());
    }
    values
}

/// X-M1 1.50: the raw profile byte 0x74e5c0 returns.
pub const fn profile_selection_byte(selection: crate::profile::ProfileSelection) -> u8 {
    use crate::profile::ProfileSelection;
    match selection { ProfileSelection::Unselected => 0, ProfileSelection::Record => 1, ProfileSelection::Other(byte) => byte }
}

/// X-M1 1.50: 0xff6b4c case 0 captures the primary statistics into 4096 bytes at 0x5ffec1b4.
const PRIMARY_STATISTICS: (u32, u32) = (0x5ffe_c1b4, 4096);

/// X-M1 1.50: 0xf3a564..0xf3a73c builds the 76-byte window request on the stack
/// and 0xf4c6e8 posts it with the completion word at byte 76 to calc 0x3c0c.
/// Bytes 56..63 come from planner words +0x48 and +0x44 (0xfa8018, 0xfa9588),
/// whose setters 0xfa8000 and 0xfa9578 have no caller, so they are zero.
/// Unwritten stack bytes 10..11, 17..19, 28..31 and 34..35 are zero here.
pub fn window_request_payload(request: crate::window::WindowRequest, completion: xm1_saclac_controllers::stock::StockCallbackWord) -> [u8; 80] {
    let mut bytes = [0; 80];
    let mut put = |offset: usize, value: &[u8]| bytes[offset..offset + value.len()].copy_from_slice(value);
    let snapshot = request.snapshot;
    put(0, &u32::from(profile_selection_byte(request.selection)).to_le_bytes());
    put(4, &request.record.first.0.to_le_bytes());
    put(8, &request.level.to_le_bytes());
    put(12, &snapshot.mode.0.to_le_bytes());
    put(16, &[snapshot.flags]);
    put(20, &snapshot.exposure[0].0.to_le_bytes());
    put(24, &snapshot.levels[0].to_le_bytes());
    put(26, &request.reference.to_le_bytes());
    put(32, &request.record.halfword.0.to_le_bytes());
    put(36, &PRIMARY_STATISTICS.0.to_le_bytes());
    put(40, &PRIMARY_STATISTICS.1.to_le_bytes());
    put(44, &snapshot.adjustment.0.to_le_bytes());
    put(48, &request.record.status.0.to_le_bytes());
    put(52, &request.record.second.0.to_le_bytes());
    put(64, &u32::from(request.refresh).to_le_bytes());
    put(68, &u32::from(request.mode.0).to_le_bytes());
    put(72, &u32::from(request.selected).to_le_bytes());
    put(76, &completion.0.to_le_bytes());
    bytes
}
