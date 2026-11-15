#![forbid(unsafe_code)]

use xm1_saclac_interface::{statistics::StatisticsSetting, stock::StockCallbackWord};

/// X-M1 f2a1b0..c0 and1039b48..c24: four record segments, then the completion word.
pub(crate) fn setting(setting: &StatisticsSetting, callback: StockCallbackWord) -> [u8; 148] {
    let source = setting.bytes();
    let mut bytes = [0; 148];
    bytes[..56].copy_from_slice(&source[88..144]);
    bytes[56..72].copy_from_slice(&source[72..88]);
    bytes[72..88].copy_from_slice(&source[56..72]);
    bytes[88..144].copy_from_slice(&source[..56]);
    bytes[144..].copy_from_slice(&callback.0.to_le_bytes());
    bytes
}
