//! How a run searches (0xf9e22c): its search mode, the channel class of its lens bounds, what it
//! hands the pipeline about the second channel, the area it measures, the level ceiling of its
//! exposure program and the frame grids it samples.

use xm1_saclac_interface::calc::SharedAddress;
use xm1_saclac_profile_records::stock::shared::group_17;
use xm1_saclac_profile_records::MeasurementSelection;

use crate::adjust::lens::LensConfiguration;
use crate::adjust::motion::exposure::{negative_scale, positive_scale};
use crate::adjust::motion::ports::{AreaGrid, BoundsQuery, FrameGeometry, LensBounds, LensCalibration, LensTables, Logarithms10, SearchFrameRequest, Tracking};
use crate::adjust::motion::queries;
use crate::adjust::motion::readings::{AreaList, RecordReadings};
use crate::adjust::rate_policy;
use crate::adjust::scene_policy::{Treatment, TreatmentInputs};
use crate::adjust::timing::LevelScale;
use crate::exposure::ports::{ProfileAccess, Properties, Property, StockMemory};
use crate::exposure::settings;
use crate::stock::motion::{self as stock};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SearchMode {
    /// Configuration 0xde bit 0, or a class the other modes do not take.
    #[default]
    Plain,
    /// Classes 1, 4 and above 6 whose channel configuration 0x928e enables.
    Channel,
    /// Classes 0, 2 and 5, and classes 3 and 6 under the load test when configuration 0xa6 is 1.
    Area,
    /// The scene treatment's second variant admits the record.
    Treated,
}

impl SearchMode {
    /// The word the pipeline reads for the mode (0xf9d49c).
    pub const fn code(self) -> u32 {
        match self {
            Self::Plain => 0,
            Self::Channel => 1,
            Self::Area => 2,
            Self::Treated => 3,
        }
    }
}

/// Which channels' lens bounds a run spans.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChannelClass {
    /// Configuration 0x946d is 0.
    #[default]
    Off,
    /// The run keeps to one channel.
    Single,
    /// The configured mask 0x9473 refuses the record's second channel.
    Masked,
    /// An armed record whose scene bit configuration 0x9474 does not set.
    Armed,
    /// Channel 0's high bound reaches at least as far as channel 1's.
    Narrow,
    /// The run's high bound is channel 1's.
    Wide,
}

impl ChannelClass {
    pub const fn code(self) -> u32 {
        match self {
            Self::Off => 0,
            Self::Single => 1,
            Self::Masked => 2,
            Self::Armed => 5,
            Self::Narrow => 11,
            Self::Wide => 12,
        }
    }
}

/// The area of the 7-column grid the run measures.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AreaCell {
    pub column: u8,
    pub row: u8,
}

impl AreaCell {
    pub const fn index(self) -> u8 { self.row.wrapping_mul(7).wrapping_add(self.column) }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SearchSetup {
    pub mode: SearchMode,
    pub channel_class: ChannelClass,
    pub retry_missing: bool,
    /// The lens snapshot's position on side 1.
    pub snapshot_position: i16,
    /// What 0xf9cdd8 answers the pipeline.
    pub channel_status: bool,
    /// The word the pipeline takes at the preparation's output +0x2c: 0, 3, 8 or 12.
    pub frame_delay: u8,
    /// Percent of the area the run samples.
    pub area_scale: u16,
    pub area: AreaCell,
    /// The contrast results fall short of the configured ratio of their reference area.
    pub below_reference: bool,
    /// The highest level the exposure program's second ranges reach.
    pub level_ceiling: i16,
    pub frames: [AreaGrid; 2],
}

/// The selection lists' readings the search setup takes besides the record's.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ListReadings {
    pub subjects: u8,
    pub single_subject: u8,
}

pub trait SearchPorts:
    Properties + ProfileAccess + LensTables + LensCalibration + LensBounds + LensConfiguration + StockMemory + LevelScale + Logarithms10 + FrameGeometry
{
}

impl<T: Properties + ProfileAccess + LensTables + LensCalibration + LensBounds + LensConfiguration + StockMemory + LevelScale + Logarithms10 + FrameGeometry> SearchPorts
    for T
{
}

/// A configured byte as a signed value (0x69ff68).
pub(crate) fn signed_byte(properties: &mut impl Properties, id: Property) -> i16 { i16::from(properties.property(id) as u8 as i8) }

const fn channel_bit(channel: u8) -> u32 {
    match channel {
        1 => 2,
        2 => 4,
        _ => 1,
    }
}

impl SearchSetup {
    /// The search setup for a run with `readings`, the listed `areas` in the search profile's
    /// frame and contrast results at `results`.
    pub fn decide(ports: &mut impl SearchPorts, readings: &RecordReadings, lists: ListReadings, areas: &AreaList, results: SharedAddress) -> Self {
        let mode = search_mode(ports, readings, lists);
        let channel_class = channel_class(ports, readings, mode);
        let snapshot: [u8; 48] = ports.profile().read_shared(readings.record, &group_17::LENS_SNAPSHOT);
        let snapshot_side = u32::from_le_bytes([snapshot[12], snapshot[13], snapshot[14], snapshot[15]]);
        let snapshot_position = ports.side_adjusted(i16::from_le_bytes([snapshot[0], snapshot[1]]), snapshot_side, 1);
        let (channel_status, frame_delay) = channel_status(ports, readings, mode, areas);
        let (area_scale, area) = match readings.class {
            2 | 5 => (readings.at_0028, selected_cell(readings)),
            0 => (100, selected_cell(readings)),
            _ => (100, AreaCell { column: 3, row: 3 }),
        };
        let ratio_scale = u32::from(settings::word(ports, stock::RATIO_CHECK_SCALE));
        let below_reference = ports.property(stock::RATIO_CHECK) != 0 && !readings.load_test && contrast_reaches(ports, results, area, mode, ratio_scale) != Some(true);
        let level_ceiling = level_ceiling(ports, readings, mode, area, results);
        let request = search_frame_request(ports, areas);
        let frames = ports.search_frames(&request);
        Self { mode, channel_class, retry_missing: false, snapshot_position, channel_status, frame_delay, area_scale, area, below_reference, level_ceiling, frames }
    }
}

fn selected_cell(readings: &RecordReadings) -> AreaCell { AreaCell { column: readings.area.column as u8, row: readings.area.row as u8 } }

/// 0xf9e22c..0xf9e330.
fn search_mode(ports: &mut impl Properties, readings: &RecordReadings, lists: ListReadings) -> SearchMode {
    if ports.property(stock::PLAIN_SEARCH) & 1 != 0 {
        return SearchMode::Plain;
    }
    let treatment = TreatmentInputs {
        active: u32::from(readings.compares_subjects),
        subjects: u32::from(lists.subjects),
        measurement_selection: readings.measurement_selection,
        scene: u32::from(readings.scene),
        class: u32::from(readings.class),
        callback_armed: u32::from(readings.armed),
        held: u32::from(readings.auxiliary),
        single_subject: lists.single_subject,
    };
    if treatment.admits(Treatment::Second) {
        return SearchMode::Treated;
    }
    match readings.class {
        0 | 2 | 5 => SearchMode::Area,
        3 | 6 if !readings.load_test => SearchMode::Plain,
        3 | 6 if ports.property(stock::LOAD_TEST_PLAIN) != 0 => SearchMode::Plain,
        3 | 6 if ports.property(stock::LOAD_TEST_AREA) == 1 => SearchMode::Area,
        3 | 6 => SearchMode::Plain,
        _ if ports.property(stock::CHANNEL_SEARCH) & channel_bit_of_two(readings.channel) != 0 => SearchMode::Channel,
        _ => SearchMode::Plain,
    }
}

/// 2 for channels 1 and 2, else 1 (0xf9d3a8).
const fn channel_bit_of_two(channel: u8) -> u32 { if matches!(channel, 1 | 2) { 2 } else { 1 } }

/// 0xf9e334..0xf9e664.
fn channel_class(ports: &mut (impl Properties + LensCalibration + LensBounds), readings: &RecordReadings, mode: SearchMode) -> ChannelClass {
    if ports.property(stock::CHANNEL_CLASSES) == 0 {
        return ChannelClass::Off;
    }
    let single = readings.lens_state != 0
        || readings.channel != 0
        || matches!(readings.class, 0 | 1 | 4)
        || readings.load_test
        || ports.property(stock::PLAIN_SEARCH) & 1 != 0
        || readings.lens as i32 > ports.property(stock::LENS_CHANNEL_LIMIT) as i32 - 1
        || readings.locked == 3;
    if single {
        return ChannelClass::Single;
    }
    let scene = stock::scene_channels(readings.scene);
    if readings.armed == 1 {
        return match scene.armed_bit {
            None => ChannelClass::Single,
            Some(bit) if ports.property(stock::ARMED_SCENE_CHANNELS) & bit != 0 => ChannelClass::Wide,
            Some(_) => ChannelClass::Armed,
        };
    }
    if readings.measurement_selection == MeasurementSelection::Record {
        return ChannelClass::Wide;
    }
    if !scene.second_channel {
        return ChannelClass::Single;
    }
    let mask = ports.property(stock::SECOND_CHANNEL_MASK);
    let refused = (matches!(readings.class, 2 | 5) && mask & 1 == 0)
        || (readings.at_005b == 1 && mask & 4 == 0)
        || (matches!(readings.class, 5 | 6) && mask & 8 == 0)
        || (mode == SearchMode::Treated && mask & 0x10 == 0)
        || (readings.at_004e == 1 && mask & 0x40 == 0)
        || (readings.scene == stock::MASKED_SCENE && mask & 0x80 == 0);
    if refused {
        return ChannelClass::Masked;
    }
    let step = ports.lowest_step(readings.lens);
    let mut high_bound = |channel| {
        ports.search_bounds(&BoundsQuery { lens: readings.lens, scene: readings.scene, channel, class: readings.class, step, reference: readings.reference, held: readings.auxiliary, lens_state: 0 }).high
    };
    if high_bound(0) < high_bound(1) { ChannelClass::Wide } else { ChannelClass::Narrow }
}

/// 0xf9e698..0xf9e868 with 0xf9ecec and 0xf9ed18.
fn channel_status(ports: &mut (impl Properties + LensConfiguration), readings: &RecordReadings, mode: SearchMode, areas: &AreaList) -> (bool, u8) {
    const OFF: (bool, u8) = (false, 12);
    if readings.at_005f == 0 || readings.load_test {
        return OFF;
    }
    let limit = signed_byte(ports, stock::CHANNEL_LEVEL_LIMIT).wrapping_mul(10);
    let mut bits = if readings.configured_level(ports) < limit { channel_bit(readings.channel) } else { 0 };
    if readings.confirmation[0] == 1 && readings.confirmation[1] == 4 {
        bits |= match readings.channel {
            1 => 0x20,
            2 => 0x40,
            _ => 0x10,
        };
    }
    if bits & ports.property(stock::CHANNEL_STATUS_MASK) == 0 {
        return OFF;
    }
    if matches!(mode, SearchMode::Plain | SearchMode::Area) && readings.class != 0 && channel_bit(readings.channel) & ports.property(stock::CHANNEL_STATUS_ENABLE) != 0 {
        return (true, 0);
    }
    let reaches_rate = mode == SearchMode::Treated
        && !matches!(readings.scene, stock::FIRST_HELD_SCENE | stock::SECOND_HELD_SCENE)
        && readings.auxiliary == 0
        && rate_policy::drive_rate(ports, false, readings.lens as i32, u32::from(areas.areas[0].at_22)).scaled >= ports.property(stock::RATE_THRESHOLD);
    let delay = if readings.scene == stock::DELAYED_SCENE { 8 } else { 3 };
    (false, if reaches_rate || readings.channel != 0 { delay } else { 0 })
}

/// Whether the measured contrast in the results reaches `scale` per mille of the reference
/// area's, scaled by the results' level offsets; `None` while the results hold no measurement
/// (0xf9d544 with 0xf9d4bc).
pub(crate) fn contrast_reaches(ports: &mut (impl Properties + StockMemory + LevelScale), results: SharedAddress, area: AreaCell, mode: SearchMode, scale: u32) -> Option<bool> {
    contrast_threshold(ports, results, area, mode, scale).map(|(measured, threshold)| measured >= threshold)
}

/// The measured contrast and the threshold it is compared with.
pub(crate) fn contrast_threshold(ports: &mut (impl Properties + StockMemory + LevelScale), results: SharedAddress, area: AreaCell, mode: SearchMode, scale: u32) -> Option<(u32, u32)> {
    if ports.read_u32(stock::results_present(results)) == 0 {
        return None;
    }
    let frame = u32::from(ports.read_u8(stock::results_frame(results)));
    let cell = u32::from(area.index()) * 5 + frame;
    let alternate = ports.property(stock::RESULT_TABLES) == 1;
    let measured = ports.read_u32(match (alternate, mode == SearchMode::Area) {
        (true, true) => stock::result_cell_contrast(results, cell, true),
        (true, false) => stock::result_frame_contrast(results, frame, true),
        (false, true) => stock::result_cell_contrast(results, cell, false),
        (false, false) => stock::result_frame_contrast(results, frame, false),
    });
    let [area_height, area_width, reference_height, reference_width, first_offset, second_offset] = stock::result_ratio_words(results, frame).map(|at| ports.read_u32(at));
    let first = u32::from(negative_scale(ports, (first_offset as i16).wrapping_neg()));
    let step_two = settings::level_step(ports, 2);
    let second = u32::from(positive_scale(ports, (second_offset as i16).wrapping_sub(step_two)));
    let area = u64::from(area_height.wrapping_mul(area_width));
    let reference = u64::from(reference_height.wrapping_mul(reference_width));
    let reduced = crate::adjust::motion::drive::quotient(u64::from(scale) * area, reference) as u32;
    let threshold = second.wrapping_mul(reduced.wrapping_mul(first) / 10) / 10;
    Some((measured, threshold))
}

/// 0xf9e9a8..0xf9eb30.
fn level_ceiling(ports: &mut (impl Properties + StockMemory + Logarithms10), readings: &RecordReadings, mode: SearchMode, area: AreaCell, results: SharedAddress) -> i16 {
    if mode == SearchMode::Treated {
        return 0;
    }
    if ports.read_u32(stock::results_present(results)) == 0 || readings.class == 0 {
        return settings::level(ports, stock::LEVEL_CEILING_DEFAULT).wrapping_mul(10);
    }
    let frame = u32::from(ports.read_u8(stock::results_frame(results)));
    let cell = u32::from(area.index()) * 5 + frame;
    let alternate = ports.property(stock::LEVEL_CEILING_TABLE) != 0;
    let contrast = ports.read_u32(stock::result_cell_contrast(results, cell, alternate));
    let x = (ports.log10(contrast as i32 as f32) * 1000.0) as i32;
    let [low, high, low_level, high_level] = if queries::level_peaked(ports, results) { stock::PEAKED_CEILING } else { stock::LEVEL_CEILING_BAND };
    let low = i32::from(settings::word(ports, low));
    let high = i32::from(settings::word(ports, high));
    let low_level = settings::level(ports, low_level).wrapping_mul(10);
    let high_level = settings::level(ports, high_level).wrapping_mul(10);
    if high == low || x <= low {
        return low_level;
    }
    if x >= high {
        return high_level;
    }
    let span = (x - low).wrapping_mul(i32::from(high_level) - i32::from(low_level));
    (span.wrapping_div(high - low) as i16).wrapping_add(low_level)
}

/// 0xf9eb48..0xf9ec5c.
fn search_frame_request(ports: &mut impl Properties, areas: &AreaList) -> SearchFrameRequest {
    let area = areas.listed().first().map(|area| Tracking { area: *area, class: stock::area_class(area.at_1c) });
    SearchFrameRequest {
        grid: stock::SEARCH_FRAME_SETTINGS.map(|id| ports.property(id) as u16),
        area,
    }
}
