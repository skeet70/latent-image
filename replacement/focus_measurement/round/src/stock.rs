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
    match byte.0 { 0 => ProfileSelection::Unselected, 1 => ProfileSelection::Record, _ => ProfileSelection::Other }
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

/// X-M1 1.50: 0xfa72d0..0xfa72e8 reads the property keys at 0xfab110.
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
        Mode::Default => 5, Mode::RecordSelected => 0, Mode::ResourceSelected => 1,
        Mode::ProfileSelected => 2, Mode::AboveThreshold => 3, Mode::BelowThreshold => 4,
        Mode::Paused => return None,
    };
    Some(SchedulingProperty(base + offset))
}

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
