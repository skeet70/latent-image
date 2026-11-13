//! Levels the exposure limits, the motion library and the shot read against a scene's
//! configuration: response limits, the excess over a placed level, and the configured rate.

use xm1_saclac_interface::calc::SharedAddress;

use crate::exposure::ports::{Lens, Properties, StockMemory};
use crate::exposure::settings;
use crate::stock::scene_levels as stock;
use crate::stock::shared::level_curve_base;

/// The profile table's feature classes.
pub trait FeatureClasses {
    /// 0xef53d0.
    fn feature_class(&mut self, profile: u32) -> u8;
}

fn response_band(level: i16) -> u8 {
    match level {
        100.. => 0,
        0..=99 => 1,
        -100..=-1 => 2,
        -200..=-101 => 3,
        -300..=-201 => 4,
        -400..=-301 => 5,
        _ => 6,
    }
}

/// The limit the capture profile's response sets at `level` for limit `index` (0xf5f3c0).
pub fn response_limit(ports: &mut (impl Properties + FeatureClasses), capture_profile: u32, level: i16, index: u32) -> i16 {
    let class_three = ports.feature_class(capture_profile) == 3;
    let column = match index {
        0 => 3,
        1 => 2,
        2 => 1,
        _ => 0,
    };
    settings::signed_word(ports, stock::response_limit(class_three, response_band(level), column))
}

/// The excess of `level` over `placed` (0xf5f324).
pub fn excess(lens: &mut impl Lens, level: i16, placed: i16, step: u32, offset: i16) -> i16 {
    let base = lens.base_level(step);
    let raw = i32::from(level).wrapping_sub(i32::from(placed)).wrapping_add(i32::from(offset)).wrapping_sub(i32::from(base)) as i16;
    raw.clamp(-200, 200)
}

/// The excess of a scene measured against its placement (0xf5f368).
pub fn scene_excess(lens: &mut impl Lens, scene: u32, level: i16, placed: Option<i16>, step: u32, offset: i16) -> i16 {
    match placed {
        Some(placed) if stock::PLACED_SCENES.contains(&scene) => excess(lens, level, placed, step, offset),
        _ => 0,
    }
}

/// Point `point` of the level curves calc reads, counted across the four curves (0xf5f5d4).
pub fn moved_curve_point(memory: &mut impl StockMemory, point: u32, from: i16, to: i16) -> i16 {
    let at = SharedAddress(level_curve_base(0).0.wrapping_add(2 * point));
    to.wrapping_sub(from).wrapping_add(memory.read_u16(at) as i16)
}

/// The configured rate scaled by an area, whole and in tenths.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ConfiguredRate {
    pub whole: u32,
    pub tenths: u32,
    pub doubled_tenths: u32,
}

/// 0xf5eba0.
pub fn configured_rate(properties: &mut impl Properties, width: u32, height: u32) -> ConfiguredRate {
    let area = width.wrapping_mul(height);
    let [low, middle, high] = stock::CONFIGURED_RATE.map(|byte| properties.property(byte));
    let whole = high << 16 | middle << 8 | low;
    let tenths = whole / 10;
    ConfiguredRate { whole: whole.wrapping_mul(area), tenths: tenths.wrapping_mul(area), doubled_tenths: (tenths << 1).wrapping_mul(area) }
}
