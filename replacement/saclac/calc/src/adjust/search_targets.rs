//! The contrast search's targets and stop tests: its drive target for an amount, the configured
//! step and threshold readings, the peak tests over its sample series and the bias interpolation
//! between lens resources.

use xm1_saclac_interface::RecordHandle;
use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::adjust::focus_profile::LensResources;
use crate::adjust::lens::{BiasTable, LensConfiguration};
use crate::exposure::ports::{ProfileAccess, Properties};
use crate::exposure::settings;
use crate::stock::adjust as stock;

/// The configured minimum step, at least `minimum` (0xfa7614).
pub fn minimum_step(properties: &mut impl Properties, minimum: u32) -> u32 { properties.property(stock::MINIMUM_STEP).max(minimum) }

/// The drive target for `amount` on the lens resource `channel`, within `minimum..=maximum`
/// (0xfa7ce0). A zero channel value or curve contributes no offset.
pub fn drive_target(
    properties: &mut impl Properties,
    configuration: &mut impl LensConfiguration,
    lens: &mut impl LensResources,
    channel: i32,
    amount: u32,
    minimum: i32,
    maximum: i16,
) -> i16 {
    let factor = properties.property(stock::TARGET_FACTOR);
    let base_channel = properties.property(stock::TARGET_BASE_CHANNEL) as i32;
    let baseline = configuration.channel_value(base_channel).wrapping_mul(100);
    let rate = (amount.wrapping_mul(factor) / 100).wrapping_mul(baseline).checked_div(configuration.channel_value(channel)).unwrap_or(0);
    let duration = u32::from(settings::word(properties, stock::TARGET_DURATION));
    let value = configuration.channel_value(channel);
    let reach = (duration.wrapping_mul(rate) / 10_000).wrapping_mul(value.wrapping_mul(value));
    let curve = configuration.channel_curve(channel);
    let bias = configuration.bias(channel, BiasTable::Area);
    let offset = (reach.checked_div(curve.wrapping_mul(10_000)).unwrap_or(0) as u16).wrapping_add(bias as u16);
    let flags = lens.flags();
    let word = configuration.selection_word(channel, flags, offset.wrapping_mul(2) as i16);
    let mode = lens.reference();
    let target = configuration.remap(word, channel, mode).max(minimum) as i16;
    target.min(maximum)
}

/// Whether `base` plus `first` times `second` percent plus the configured margin stays below
/// `limit` (0xfa9320).
pub fn below_threshold(properties: &mut impl Properties, limit: u32, first: u32, base: u32, second: u32) -> bool {
    let margin = properties.property(stock::THRESHOLD_MARGIN).wrapping_mul(10);
    base.wrapping_add(first.wrapping_mul(second) / 100).wrapping_add(margin) < limit
}

/// The run of samples falling to the latest one: how many steps back the peak lies, the peak, and
/// the fall from it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Fall {
    pub steps: u8,
    pub peak: u32,
    pub depth: u32,
}

/// The fall to the last sample, `None` when the sample before it is not higher.
pub fn fall(samples: &[u32]) -> Option<Fall> {
    let (&latest, earlier) = samples.split_last()?;
    let rising_back = earlier.iter().rev().scan(latest, |next, &sample| {
        let higher = sample > *next;
        *next = sample;
        higher.then_some(sample)
    });
    let (steps, peak) = rising_back.enumerate().last().map(|(index, peak)| (index + 1, peak))?;
    Some(Fall { steps: steps as u8, peak, depth: peak.wrapping_sub(latest) })
}

/// Whether the series has passed its peak: a fall of at least the configured length, or a
/// shorter one deeper than `scale` percent of the peak plus the configured margin (0xfa936c).
/// `mode_one` is the search's mode word at 1, which reads the second length.
pub fn passed_peak(properties: &mut impl Properties, samples: &[u32], scale: u32, mode_one: bool) -> (bool, Option<u32>) {
    let length = properties.property(if mode_one { stock::PEAK_LENGTH_MODE_ONE } else { stock::PEAK_LENGTH });
    let margin = properties.property(stock::THRESHOLD_MARGIN).wrapping_mul(10);
    let Some(fall) = fall(samples) else {
        return (false, None);
    };
    if u32::from(fall.steps) >= length {
        return (true, Some(fall.peak));
    }
    let share = (u64::from(scale) * u64::from(fall.peak) / 100) as u32;
    (margin.wrapping_add(share) <= fall.depth, Some(fall.peak))
}

/// Whether the series has fallen from its peak by at least the configured length and the
/// configured depth in hundreds (0xfa9488).
pub fn fell_from_peak(properties: &mut impl Properties, samples: &[u32]) -> bool {
    let length = properties.property(stock::FALL_LENGTH);
    let depth = properties.property(stock::FALL_DEPTH);
    fall(samples).is_some_and(|fall| u32::from(fall.steps) >= length && fall.depth >= depth.wrapping_mul(100))
}

/// The record's scene kind and its follow word, bit 0 of the word set while `enabled` (0xfaa7b0).
pub fn scene_and_follow(ports: &mut impl ProfileAccess, record: RecordHandle, enabled: bool) -> (u32, u16) {
    let scene = u32::from(ports.profile().header(record, SharedHeaderByte::At31));
    let follow: u16 = ports.profile().read(record, &group_00::AT_002E);
    (scene, if enabled { follow | 1 } else { follow & !1 })
}

/// The lens bias at `position` interpolated from `resource` toward the configured step's resource,
/// up when `ascending`, within the configured resources (0xfaa7f4). A zero position at the nearer
/// resource interpolates nothing.
pub fn interpolated_bias(properties: &mut impl Properties, configuration: &mut impl LensConfiguration, resource: u32, ascending: bool, position: i32) -> i16 {
    let current = resource as u8;
    let (near, far, table) = if ascending {
        let step = properties.property(stock::ascending_step(resource)) as u8;
        let count = configuration.resource_count();
        let near = if i32::from(count) <= i32::from(step as i8) + i32::from(current as i8) { (!step).wrapping_add(count) } else { current };
        (near as i8, step.wrapping_add(near) as i8, BiasTable::Ascending)
    } else {
        let step = properties.property(stock::descending_step(resource)) as u8;
        let near = if (current as i8) < (step as i8) { step } else { current };
        (near as i8, near.wrapping_sub(step) as i8, BiasTable::Descending)
    };
    let far_bias = i32::from(configuration.bias(i32::from(far), table));
    let near_bias = i32::from(configuration.bias(i32::from(near), table));
    let far_position = i32::from(configuration.biased_position(i32::from(far), table));
    let near_position = i32::from(configuration.biased_position(i32::from(near), table));
    let scaled = far_position.wrapping_mul(position.wrapping_sub(near_bias)).checked_div(near_position).unwrap_or(0);
    far_bias.wrapping_sub(position).wrapping_add(scaled) as i16
}

/// Whether the scene admits the adjustment: never unless `enabled` and setting 0xa7e4 bit 0,
/// always in mode 1, else for the listed scenes (0xfab250).
pub fn scene_admitted(properties: &mut impl Properties, mode_one: bool, scene: u32, enabled: bool) -> bool {
    const ADMITTED: [u32; 37] = [0, 1, 2, 3, 4, 8, 9, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 26, 27, 30, 31, 33, 35, 36, 38, 39, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 57];
    enabled && properties.property(stock::SCENE_ADMISSION) & 1 != 0 && (mode_one || ADMITTED.contains(&scene))
}

/// The extent threshold for the lens's mapped value: below 400, 560, 800, and from 800 (0xfab43c).
pub fn extent_threshold(properties: &mut impl Properties, configuration: &mut impl LensConfiguration, key: u32) -> i16 {
    let band = match configuration.mapped_value(key) {
        0..=399 => 0,
        400..=559 => 1,
        560..=799 => 2,
        _ => 3,
    };
    properties.property(stock::EXTENT_THRESHOLDS[band]) as i16
}
