use std::collections::HashMap;

use crate::channel::Channel;
use crate::fake::Area;
use crate::limits::*;
use crate::ports::{Derivations, ExportQuery, RangeQuery};
use crate::table::Step;

#[derive(Default)]
struct Stock {
    exported: HashMap<i32, i16>,
    scaled: u16,
    channel_scale: u16,
    table_scale: u16,
    positions: HashMap<i16, u32>,
    values: HashMap<(u32, i32), i16>,
    reference: i16,
    applied: Vec<(i16, Channel)>,
    exports: Vec<(Channel, ExportQuery)>,
    scaled_queries: Vec<(Channel, u32, u32)>,
    lens: Vec<(u32, u32, u32)>,
    limits: Vec<(u32, [u32; 11])>,
}

impl Derivations for Stock {
    fn export_word(&mut self, channel: Channel, query: ExportQuery) -> i16 {
        self.exports.push((channel, query));
        self.exported.get(&channel.0).copied().unwrap_or(0)
    }
    fn adjust_direction(&mut self, word: i16, _: u32, mode: u32) -> i16 { word + mode as i16 }
    fn scaled_limit(&mut self, channel: Channel, first: u32, scale: u32) -> u16 {
        self.scaled_queries.push((channel, first, scale));
        self.scaled
    }
    fn channel_scale(&mut self, _: Channel) -> u16 { self.channel_scale }
    fn table_scale(&mut self, _: u32, _: Channel) -> u16 { self.table_scale }
    fn curve_position(&mut self, level: i16, _: Channel) -> u32 { self.positions.get(&level).copied().unwrap_or(50) }
    fn curve_value(&mut self, position: u32, channel: Channel) -> i16 { self.values.get(&(position, channel.0)).copied().unwrap_or(0) }
    fn channel_word(&mut self, _: Channel, _: u32, _: u32) -> i16 { 0 }
    fn reference_word(&mut self, _: u32, _: Channel) -> i16 { self.reference }
    fn apply_reference(&mut self, word: i16, channel: Channel) { self.applied.push((word, channel)); }
    fn lens_table(&mut self, selector: u32, first: u32, second: u32) -> u32 {
        self.lens.push((selector, first, second));
        0x77
    }
    fn lens_limits(&mut self, table: u32, words: [u32; 11]) -> u32 {
        self.limits.push((table, words));
        5
    }
}

#[test]
fn limits_are_read_by_mode_class_kind_and_side() {
    let area = Area::default().with_word(0x3e145a, 11).with_word(0x3e14c5, 22).with_word(0x3e146b, 33).with_word(0x3e14f8, 44);
    assert_eq!(limit(&area, Limit::Word, Kind::First, Side::One, 5), 11, "mode 5 is in the remaining class");
    assert_eq!(limit(&area, Limit::Word, Kind::Second, Side::Other, 8), 22);
    assert_eq!(limit(&area, Limit::Word, Kind::First, Side::One, 9), 33);
    assert_eq!(limit(&area, Limit::Word, Kind::Second, Side::One, 12), 44);
    assert_eq!(ModeClass::of(3), ModeClass::Modes1To4);
    assert_eq!(ModeClass::of(0), ModeClass::Others);
}

#[test]
fn shifted_limits_shift_by_the_adjustment_and_optional_limits_read_zero_as_unlimited() {
    let area = Area::default().with_half(0x3e145e, 3).with(0x3e162f, 4);
    assert_eq!(limit(&area, Limit::ShiftedHalf, Kind::First, Side::One, 0), 48);
    assert_eq!(limit(&area, Limit::OptionalShiftedNextHalf, Kind::First, Side::One, 0), 0xffff);
    assert_eq!(limit(&area, Limit::OptionalWord, Kind::First, Side::One, 0), 0xffff);
    let area = area.with_half(0x3e1676, 1).with_half(0x3e1674, 2).with_half(0x3e1630, 7);
    assert_eq!(limit(&area, Limit::OptionalShiftedNextHalf, Kind::First, Side::One, 0), 16);
    assert_eq!(limit(&area, Limit::OptionalShiftedHalf, Kind::First, Side::One, 0), 32);
    assert_eq!(limit(&area, Limit::Half, Kind::First, Side::One, 0), 7, "unshifted");
    assert_eq!(limit(&area.with(0x3e162f, 40), Limit::ShiftedHalf, Kind::First, Side::One, 0), 0, "shifted out");
    assert_eq!(shifted_floor(&Area::default().with_half(0x3e15a6, 5).with(0x3e162f, 1)), 10);
}

#[test]
fn an_unconfigured_channel_reads_the_first_limit() {
    let area = Area::default().with(0x3e0f80, 2).with_half(0x3e0a6b, 7).with_half(0x3e0a6d, 9).with_half(0x3e09a4, (-3i16) as u16);
    assert_eq!(channel_limit(&area, ChannelLimit::Primary, Channel(1)), (9, true));
    assert_eq!(channel_limit(&area, ChannelLimit::Primary, Channel(2)), (7, false));
    assert_eq!(channel_limit(&area, ChannelLimit::Secondary, Channel(1)), (-3, true));
}

#[test]
fn a_scaled_value_divides_by_the_table_scale_or_one() {
    let mut stock = Stock { channel_scale: 3, table_scale: 4, ..Stock::default() };
    assert_eq!(scaled_value(&mut stock, Channel(0), 1, 20), 150);
    stock.table_scale = 0;
    assert_eq!(scaled_value(&mut stock, Channel(0), 1, 20), 600);
}

fn query(mode: LimitMode) -> LimitQuery {
    LimitQuery { channel: Channel(1), first: 2, value: 300, divisor: 100, mode, kind: Kind::First, side: Side::One, limit_mode: 0 }
}

#[test]
fn a_selected_limit_caps_the_value_and_the_scaled_limit_and_converts_within_floor_and_mode_limit() {
    let area = Area::default().with(0x3e144d, 20).with_half(0x3e1445, 40).with_half(0x3e15a6, 100).with_half(0x3e1676, 10_000);
    let mut stock = Stock { scaled: 60, ..Stock::default() };
    assert_eq!(selected_limit(&area, &mut stock, query(LimitMode::Capped)), (60, 4000), "40 * 10000 / 100");
    assert_eq!(stock.scaled_queries, [(Channel(1), 2, 200)], "the value is capped at 20 * 10");
    let bounded = LimitMode::Bounded { upper: 30, lower: 25 };
    assert_eq!(selected_limit(&area, &mut stock, query(bounded)).1, 3000);
    stock.scaled = 1;
    assert_eq!(selected_limit(&area, &mut stock, query(LimitMode::Capped)).1, 100, "at least the floor");
    let low_mode_limit = area.with_half(0x3e1676, 50);
    stock.scaled = 60;
    assert_eq!(selected_limit(&low_mode_limit, &mut stock, query(LimitMode::Capped)).1, 50);
}

#[test]
fn an_uncapped_limit_converts_the_scaled_limit_against_its_own_mode_limit() {
    let area = Area::default().with(0x3e144d, 20).with_half(0x3e1445, 40).with_half(0x3e1674, 7000);
    let mut stock = Stock { scaled: 60, ..Stock::default() };
    assert_eq!(selected_limit(&area, &mut stock, query(LimitMode::Uncapped)), (60, 6000));
    assert_eq!(stock.scaled_queries, [(Channel(1), 2, 300)]);
}

#[test]
fn the_reference_word_feeds_its_application() {
    let mut stock = Stock { reference: -12, ..Stock::default() };
    apply_reference(&mut stock, Channel(3), 1);
    assert_eq!(stock.applied, [(-12, Channel(3))]);
}

fn position_query() -> PositionQuery { PositionQuery { first: 4, export_first: 5, direction_mode: 0, export_second: 6 } }

#[test]
fn curve_positions_keep_lower_below_and_upper_above_the_centre() {
    let area = Area::default();
    let mut stock = Stock { scaled: 40, ..Stock::default() };
    stock.exported.insert(2, 10);
    stock.positions.extend([(90, 60), (110, 70), (70, 40)]);
    let positions = curve_positions(&area, &mut stock, 100, Channel(2), position_query());
    assert_eq!(positions, CurvePositions { centre: 60, lower: 59, upper: 61 });
    assert_eq!(stock.exports, [(Channel(2), ExportQuery { selector: 0, first: 5, second: 6 })]);
    stock.positions.extend([(90, 0)]);
    assert_eq!(curve_positions(&area, &mut stock, 100, Channel(2), position_query()), CurvePositions { centre: 0, lower: 0, upper: 40 });
    stock.positions.extend([(90, 150), (70, 140)]);
    assert_eq!(curve_positions(&area, &mut stock, 100, Channel(2), position_query()).upper, 100);
    assert_eq!(curve_positions(&area.with(0x3e0009, 1), &mut stock, 100, Channel(2), position_query()), CurvePositions::default());
}

#[test]
fn interpolated_positions_read_the_channel_alone_without_a_next_channel_or_fraction() {
    let area = Area::default().with(0x3e0f80, 2);
    let mut stock = Stock::default();
    let alone = curve_positions(&area, &mut Stock::default(), 0, Channel(1), position_query());
    assert_eq!(interpolated_curve_positions(&area, &mut stock, 0, Channel(1), 0x8000, position_query()), Some(alone));
    assert_eq!(interpolated_curve_positions(&area, &mut stock, 0, Channel(0), 0, position_query()), Some(alone));
    assert_eq!(interpolated_curve_positions(&area.with(0x3e0009, 1), &mut stock, 0, Channel(0), 1, position_query()), None);
}

#[test]
fn interpolated_positions_average_the_two_channels() {
    let area = Area::default().with(0x3e0f80, 2);
    let mut stock = Stock::default();
    stock.exported.insert(1, 4);
    stock.values.extend([((50, 0), 10), ((50, 1), 30)]);
    // Halfway, the blended word is 2 and the span 20; levels -1 and 1 sit 2 and 4 under the
    // channels' adjusted words 1 and 5.
    stock.positions.extend([(-2, 20), (-4, 40)]);
    let positions = interpolated_curve_positions(&area, &mut stock, 0, Channel(0), 0x8000, position_query()).unwrap();
    assert_eq!(positions.centre, 30);
}

#[test]
fn a_channel_step_word_adds_the_words_relative_to_step_zero() {
    let area = Area::default().with(0x3e0f82, 4).with_half(0x3f2400 + 31 * 2 + 2 * 2, 70).with_half(0x3f2400 + 31 * 2, 20);
    let area = area.with_half(0x3f3d00 + 2 * 11, 9).with_half(0x3f3d02 + 2 * 11, 9);
    let mut stock = Stock::default();
    let query = WordQuery { level: 0, channel: Channel(1), step: Step(2), export_first: 0, export_second: 0, direction_mode: 0 };
    assert_eq!(channel_step_word(&area, &mut stock, query), 50 + 20, "no excess reads step 0's word");
    let spaced = area.with_half(0x3e2643 + 2, 10);
    let query = WordQuery { level: 5, ..query };
    assert_eq!(channel_step_word(&spaced, &mut stock, query), 50 + 9, "an excess under half a spacing reads the first reference");
}

#[test]
fn a_lens_range_routes_its_words_to_the_two_tables() {
    let mut stock = Stock::default();
    let words = RangeQuery(core::array::from_fn(|index| index as u32));
    assert_eq!(lens_range(&mut stock, words), 5);
    assert_eq!(stock.lens, [(2, 4, 12)]);
    assert_eq!(stock.limits, [(0x77, [3, 0, 1, 5, 6, 7, 10, 8, 11, 9, 13])]);
}
