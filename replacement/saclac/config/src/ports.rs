#![forbid(unsafe_code)]

//! Stock configuration derivations other tasks also reach. They stay stock behind the boundary;
//! the composition root binds this trait to their entries.

use crate::channel::Channel;

/// 0x7498e0's arguments after the channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExportQuery {
    pub selector: u32,
    pub first: u32,
    pub second: u32,
}

/// The lens range words 0x74c568 takes, in its argument order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RangeQuery(pub [u32; 14]);

pub trait Derivations {
    /// 0x7498e0.
    fn export_word(&mut self, channel: Channel, query: ExportQuery) -> i16;
    /// 0x747aac.
    fn adjust_direction(&mut self, word: i16, direction: u32, mode: u32) -> i16;
    /// 0x749254.
    fn scaled_limit(&mut self, channel: Channel, first: u32, scale: u32) -> u16;
    /// 0x748818.
    fn channel_scale(&mut self, channel: Channel) -> u16;
    /// 0x74888c.
    fn table_scale(&mut self, step: u32, channel: Channel) -> u16;
    /// 0x746120: the position of `level` on the channel's curve, 0..=100.
    fn curve_position(&mut self, level: i16, channel: Channel) -> u32;
    /// 0x7461a8.
    fn curve_value(&mut self, position: u32, channel: Channel) -> i16;
    /// 0x747408.
    fn channel_word(&mut self, channel: Channel, selector: u32, table: u32) -> i16;
    /// 0x747584 with its fixed zero arguments.
    fn reference_word(&mut self, mode: u32, channel: Channel) -> i16;
    /// 0x749be4 with its fixed arguments.
    fn apply_reference(&mut self, word: i16, channel: Channel);
    /// 0x7462e8.
    fn lens_table(&mut self, selector: u32, first: u32, second: u32) -> u32;
    /// 0x74b05c.
    fn lens_limits(&mut self, table: u32, words: [u32; 11]) -> u32;
}
