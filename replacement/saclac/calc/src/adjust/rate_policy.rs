//! The round's and the shot's rate readings: the configured factor, the rate kind code, the
//! drive rate for an amount, the rate policy a selector and mode choose and its settings, the
//! profile curve and threshold readings, and the configuration words the shot exports.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::stock::shared::group_22;

use crate::adjust::configuration::ProfileStore;
use crate::adjust::lens::LensConfiguration;
use crate::adjust::motion::queries::round_tenths;
use crate::exposure::ports::{CaptureProfiles, Properties};
use crate::exposure::settings;
use crate::stock::adjust as stock;
use crate::stock::adjust::Reading;

/// `value` times the configured 24-bit factor (0xfa6590).
pub fn scaled_by_configured_factor(properties: &mut impl Properties, value: u32) -> u32 {
    let [high, middle, low] = stock::CONFIGURED_FACTOR.map(|id| properties.property(id));
    value.wrapping_mul(high << 16 | middle << 8 | low)
}

/// A rate kind, 0 to 4, and its code 1 to 5.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RateKind(u8);

impl RateKind {
    /// Kinds above 4 are kind 0.
    pub const fn new(kind: u32) -> Self {
        match kind {
            1..=4 => Self(kind as u8),
            _ => Self(0),
        }
    }

    pub const fn kind(self) -> u8 { self.0 }

    /// 0xfa6854.
    pub const fn code(self) -> u32 { self.0 as u32 + 1 }

    /// Codes outside 2 to 5 are kind 0 (0xfa68ac).
    pub const fn from_code(code: u32) -> Self {
        match code {
            2..=5 => Self(code as u8 - 1),
            _ => Self(0),
        }
    }
}

/// The share of the configured rate an amount takes, the rate it drives the lens at, and that rate
/// scaled by the configured period.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DriveRate {
    pub share: u32,
    pub rate: u32,
    pub scaled: u32,
}

/// The drive rate for `amount` on `channel` against channel 0, with the second factor when
/// `second_factor` (0xfa6934). A zero channel value gives rate 0.
pub fn drive_rate(properties: &mut impl Properties, configuration: &mut impl LensConfiguration, second_factor: bool, channel: i32, amount: u32) -> DriveRate {
    let factor = properties.property(if second_factor { stock::RATE_SECOND_FACTOR } else { stock::TARGET_FACTOR });
    let share = amount.wrapping_mul(factor) / 100;
    let value = configuration.channel_value(channel);
    let base = configuration.channel_value(0);
    let rate = base.wrapping_mul(100).wrapping_mul(share).checked_div(value).unwrap_or(0);
    let steps = properties.property(stock::RATE_STEPS);
    let period = if factor.wrapping_mul(steps) == 0 {
        10
    } else {
        let span = u64::from(properties.property(stock::RATE_SPAN)) * 100_000;
        let steps = u64::from(properties.property(stock::RATE_STEPS) as u8);
        (span / (steps * u64::from(factor))) as i32
    };
    let scaled = rate.wrapping_mul(round_tenths(period) as u32) / 10_000;
    DriveRate { share, rate, scaled }
}

/// Which of eight rate policies a selector and the round's mode choose (0xfa6f54).
pub const fn rate_policy(selector_one: bool, mode: u32) -> u8 {
    if selector_one {
        match mode {
            22 | 4 | 6 => 4,
            23 | 15 => 5,
            24 | 5 => 6,
            25 | 16 => 7,
            _ => 2,
        }
    } else {
        match mode {
            19 | 2 | 26 | 17 | 27 => 1,
            _ => 0,
        }
    }
}

/// The policy's setting in the first or the second group (0xfa7a7c).
pub fn policy_setting(properties: &mut impl Properties, selector_one: bool, mode: u32, second_group: bool) -> u32 {
    properties.property(stock::policy_setting(rate_policy(selector_one, mode), second_group))
}

/// The round's rate settings for a policy, which it keeps at +0x40, +0x3e, +0x3c and +0x3a
/// (0xfa7aa4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RateSettings {
    pub byte: u8,
    /// In tenths.
    pub first: i16,
    pub second: i16,
    pub base: u16,
}

pub fn rate_settings(properties: &mut impl Properties, selector_one: bool, mode: u32, alternate: bool, kind_two: bool) -> RateSettings {
    let [byte, first, second] = stock::rate_settings(rate_policy(selector_one, mode), alternate, kind_two);
    let byte = properties.property(byte) as u8;
    let first = (properties.property(first) as i16).wrapping_mul(10);
    let second = (properties.property(second) as i16).wrapping_mul(10);
    RateSettings { byte, first, second, base: properties.property(stock::RATE_BASE) as u16 }
}

/// 0 or 1 by whether `selector` is 1 at a level from 100, else 2 or 0 (0xfa7b84).
pub const fn level_selection(selector: u32, level: i16) -> u8 {
    match (level >= 100, selector == 1) {
        (true, true) => 0,
        (true, false) => 1,
        (false, true) => 2,
        (false, false) => 0,
    }
}

/// ProfileGeometry's configuration-keyed profile curve (0xef3dac).
pub trait ProfileCurves {
    fn profile_curve(&mut self, profile: i32, class: u8) -> u32;
}

/// The profile's curve for its class: 3 when `forced`, else by the kind's bit (0xfa7e20).
pub fn profile_curve(curves: &mut impl ProfileCurves, profile: i32, forced: bool, kind: u32) -> u32 {
    let bit = if kind < 32 { 1u32 << kind } else { 0 };
    let class = if forced {
        3
    } else if bit & 0x0c08_0004 != 0 {
        1
    } else if bit & 0x8004_0002 != 0 {
        2
    } else if bit & 0x8 != 0 {
        3
    } else {
        0
    };
    curves.profile_curve(profile, class)
}

/// The policy flag: by the round's mode for selector 1, else the setting the profile's
/// sensitivity class names, 0 when none (0xfa8078).
pub fn policy_flag(ports: &mut (impl Properties + CaptureProfiles), selector_one: bool, mode: u32, profile: u32) -> u32 {
    let class = ports.sensitivity_class(profile);
    let id = if selector_one { Some(stock::policy_flag_by_mode(mode)) } else { stock::policy_flag_by_class(class) };
    id.map_or(0, |id| ports.property(id))
}

/// Copies the snapshot's current halfwords over its previous ones, for a record with kind byte
/// +0x34 of 8 to 10 whose snapshot is unlocked (0xfa80f4).
pub fn rotate_snapshot(ports: &mut impl ProfileStore, record: RecordHandle) {
    let kind: u8 = ports.profile().read(record, &group_00::AT_0034);
    if !(8..=10).contains(&kind) {
        return;
    }
    let mut snapshot: [u8; 20] = ports.profile().read_shared(record, &group_22::AT_0180);
    if snapshot[16..20] != [0; 4] {
        return;
    }
    snapshot.copy_within(6..12, 0);
    let _refused = ports.records().write_shared(record, &group_22::AT_0180, snapshot);
}

/// The configuration words the shot exports, as (word index, value) in the order stock stores
/// them (0xfa89fc).
pub fn configuration_words(properties: &mut impl Properties) -> impl Iterator<Item = (usize, u32)> + '_ {
    stock::CONFIGURATION_WORDS.iter().map(move |&(index, reading)| {
        let value = match reading {
            Reading::Byte(id) => properties.property(id),
            Reading::Word(id) => u32::from(settings::word(properties, id)),
            Reading::SignedWord(id) => settings::signed_word(properties, id) as i32 as u32,
            Reading::SignedByte(id) => properties.property(id) as i8 as i32 as u32,
            Reading::Zero => 0,
        };
        (index, value)
    })
}

/// The primary scale setting in tenths; an index above 7 reads none (0xfabf48).
pub fn primary_scale(properties: &mut impl Properties, index: u8) -> i16 {
    stock::primary_scale(index).map_or(0, |id| (properties.property(id) as i16).wrapping_mul(10))
}

/// The profile-adjustment layer's level for a profile (0xf78d24).
pub trait ProfileLevels {
    fn profile_level(&mut self, profile: u32, level: i32, reference: i32) -> i32;
}

/// Whether the profile's level is `level` (0xfaab18).
pub fn profile_level_is(levels: &mut impl ProfileLevels, level: i32, base: i32, reference: i32, profile: u32) -> bool { levels.profile_level(profile, base, reference) == level }
