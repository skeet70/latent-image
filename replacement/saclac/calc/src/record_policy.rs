//! Decisions the shot's calculation, grid and absorb steps take from the bound record and the
//! configuration.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::{group_00, group_05};
use xm1_saclac_profile_records::stock::shared::group_41;
use xm1_saclac_profile_records::{Index, IntervalTracking, MeasurementSelection, SharedHeaderByte};

use crate::adjust::focus_profile::level_band;
use crate::exposure::levels::measured_level;
use crate::exposure::ports::{ProfileAccess, Properties};
use crate::exposure::settings;
use crate::policy::{ExposureMode, SelectionKind};
use crate::stock::property::ADJUSTMENT_FLAGS;
use crate::stock::record_policy::{self as stock, position_terms};

/// The half-press mode that selects the secondary exposure modes.
const SECONDARY_HALF_PRESS: u32 = 5;
/// The half-press mode that selects the adjustment exposure modes.
const ADJUSTMENT_HALF_PRESS: u32 = 6;

/// The record kind whose mode position is the second of its table.
const SECOND_POSITION_KIND: u8 = 5;

/// Entries of the record's shared lens table.
const CURRENT_LENS_PAIR: Index<5> = Index::new(2).unwrap();
const REFERENCE_LENS_PAIR: Index<5> = Index::new(4).unwrap();

/// The exposure mode a record selects for a half press (0xf5faec).
pub fn exposure_mode(ports: &mut impl ProfileAccess, record: RecordHandle, half_press_mode: u32, refinement_pass: bool) -> ExposureMode {
    let mut profile = ports.profile();
    if profile.header(record, SharedHeaderByte::At63) != 1 {
        return ExposureMode(0);
    }
    let selected = profile.measurement_selection(record) != MeasurementSelection::Unselected;
    ExposureMode(match (half_press_mode, selected) {
        (SECONDARY_HALF_PRESS, false) => if refinement_pass { 1 } else { 2 },
        (ADJUSTMENT_HALF_PRESS, false) => 3,
        (SECONDARY_HALF_PRESS, true) => 4,
        (ADJUSTMENT_HALF_PRESS, true) => 5,
        _ => 0,
    })
}

/// What decides whether the calculation fills row data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowDataQuery {
    pub selection: MeasurementSelection,
    pub kind: SelectionKind,
    /// The word the exposure evaluation left for the selection; only 1 admits row data.
    pub evaluation: u32,
    /// The record's grid layout byte (+0x3d).
    pub layout: u8,
    pub tracking: IntervalTracking,
}

/// 0xf60bc8.
pub fn row_data_allowed(properties: &mut impl Properties, query: &RowDataQuery) -> bool {
    properties.property(stock::ROW_DATA) & 3 != 0
        && query.tracking.0 != 0
        && query.selection != MeasurementSelection::Record
        && query.evaluation == 1
        && !matches!(query.layout, 4 | 5)
        && properties.property(ADJUSTMENT_FLAGS) & 1 == 0
        && !matches!(query.kind.0, 24 | 45 | 47 | 53..=57)
}

/// The adjustment status the calculation block carries to calc 0x3c02.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdjustmentStatus {
    Accepted,
    Refused,
}

impl AdjustmentStatus {
    pub const fn code(self) -> u8 {
        match self {
            Self::Accepted => 6,
            Self::Refused => 3,
        }
    }
}

/// 0xf60e0c: `enable` is the record's byte +0x51, `overridden` its header byte 0x63.
pub fn adjustment_status(enable: u8, kind: SelectionKind, overridden: u8) -> AdjustmentStatus {
    if kind.accepted(enable == 1, overridden != 0) { AdjustmentStatus::Accepted } else { AdjustmentStatus::Refused }
}

/// Upper and lower class codes a selection may take.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClassLimits {
    pub upper: u8,
    pub lower: u8,
}

impl ClassLimits {
    const fn from_setting(value: u32) -> Self { Self { upper: (value & 0xf) as u8, lower: (value >> 4) as u8 } }
}

/// 0xf5f700.
pub fn fixed_class_limits(properties: &mut impl Properties) -> ClassLimits {
    ClassLimits::from_setting(properties.property(stock::FIXED_CLASS_LIMITS))
}

/// The class limits of the band `level` falls in (0xf5f604).
pub fn level_class_limits(properties: &mut impl Properties, level: i16) -> ClassLimits {
    let band = level_band(properties, level);
    ClassLimits::from_setting(properties.property(stock::band_class_limits(band)))
}

/// `level` raised to step 2 of the calibrated level table (0xf612a8).
pub fn floor_level(properties: &mut impl Properties, level: i16) -> i16 { level.max(settings::level_step(properties, 2)) }

/// Image 4's lens channel mapping.
pub trait ChannelMapping {
    /// The channel a lens pair maps to for `bank` (0x74714c).
    fn channel(&mut self, pair: [u32; 2], bank: u32) -> u32;
    /// 0x746864.
    fn mapped_value(&mut self, channel: u32, bank: u32) -> u32;
}

fn lens_pair(ports: &mut impl ProfileAccess, record: RecordHandle, entry: Index<5>) -> [u32; 2] {
    let bytes = ports.profile().read_shared(record, &group_41::AT_0974.at(entry));
    let word = |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    [word(0), word(4)]
}

/// Whether the channel the record's current lens pair maps to for `bank` maps to another value
/// than `previous` does (0xf5fa24).
pub fn channel_changed(ports: &mut (impl ProfileAccess + ChannelMapping), record: RecordHandle, bank: u32, previous: u32) -> bool {
    let pair = lens_pair(ports, record, CURRENT_LENS_PAIR);
    let channel = ports.channel(pair, bank);
    ports.mapped_value(channel, bank) != ports.mapped_value(previous, bank)
}

/// The channel of the record's current lens pair and whether it maps to another value than the
/// reference entry's channel maps to in the reference entry's bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceComparison {
    pub channel: u32,
    pub changed: bool,
}

/// 0xf5fa7c. The reference entry holds its bank first and its channel second.
pub fn compare_reference(ports: &mut (impl ProfileAccess + ChannelMapping), record: RecordHandle, bank: u32) -> ReferenceComparison {
    let pair = lens_pair(ports, record, CURRENT_LENS_PAIR);
    let channel = ports.channel(pair, bank);
    let current = ports.mapped_value(channel, bank);
    let [reference_bank, reference_channel] = lens_pair(ports, record, REFERENCE_LENS_PAIR);
    ReferenceComparison { channel, changed: current != ports.mapped_value(reference_channel, reference_bank) }
}

/// Image 4's adjustment position (0x740c3c).
pub trait AdjustmentPosition {
    fn apply(&mut self, selector: u32, position: i16, unmeasured: bool);
}

/// Hands the record's mode position to image 4 (0xf60e28).
pub fn apply_mode_position(ports: &mut (impl ProfileAccess + Properties + AdjustmentPosition), record: RecordHandle) {
    let kind = ports.profile().read(record, &group_00::AT_0032);
    let measured = measured_level(ports, record);
    let element = if kind == SECOND_POSITION_KIND { stock::SECOND_POSITION } else { stock::FIRST_POSITION };
    let position = position_terms(&ports.profile().read(record, &group_05::AT_016C_168.at(element)));
    if position.enabled.iter().all(|&enabled| enabled) {
        let sum = position.terms.iter().fold(0u16, |sum, &term| sum.wrapping_add(term));
        ports.apply(position.selector, sum as i16, measured == 0);
    }
}
