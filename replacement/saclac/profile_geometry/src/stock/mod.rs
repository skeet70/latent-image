//! The stock boundary: image-5 table and state addresses and layouts, and the stock entries
//! this layer calls.

pub mod tables;

pub use tables::StockTables;

use crate::numeric::FloatLog;

const LOG_FLOAT: usize = 0x894778;

/// The runtime's logarithm, entered with the value's bits in r0 as the soft-float ABI passes it.
#[derive(Clone, Copy, Debug, Default)]
pub struct StockFloatLog;

impl FloatLog for StockFloatLog {
    fn ln(&mut self, value: f32) -> f32 {
        let entry: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(LOG_FLOAT as *const ()) };
        f32::from_bits(entry(value.to_bits()))
    }
}
