//! What the recognition reaches outside the library.

use crate::exposure::ports::{ProfileAccess, Properties, StockMemory};

/// The record words the subject query reads (0xf4f18c).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SubjectRequest {
    /// The record's chosen word (0x752714 through 0xf4f16c).
    pub chosen: u32,
    pub at_0164: u32,
    pub at_0168: u32,
    /// Record word 0: the row scale's numerator and denominator.
    pub row_scale: [u16; 2],
}

/// The capture layer's subject query on a 640 by 480 frame (0xeedc84).
pub trait SubjectQuery {
    /// The number of subjects found, `None` when the query refuses.
    fn subjects(&mut self, request: &SubjectRequest) -> Option<u32>;
}

/// The lens's steadiness report (0xee9728).
pub trait Steadiness {
    fn steady(&mut self) -> bool;
}

/// The debug overlay's text lines (0x1110918).
pub trait Overlay {
    /// `text` up to 24 bytes, at `column` 1..=32 and `row` 1..=10.
    fn text(&mut self, text: &[u8], column: u8, row: u8, foreground: u8, background: u8);
}

pub trait DetectorPorts: Properties + StockMemory + ProfileAccess + SubjectQuery + Steadiness + Overlay {}

impl<T: Properties + StockMemory + ProfileAccess + SubjectQuery + Steadiness + Overlay> DetectorPorts for T {}
