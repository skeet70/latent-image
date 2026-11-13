#![forbid(unsafe_code)]

//! Values of the named fields.

use core::num::NonZeroU32;

use xm1_saclac_interface::stock::StockCallbackWord;

use crate::field::Value;
use crate::memory::{Location, Memory};

/// A value stored as one unsigned scalar.
macro_rules! stored_as {
    ($($type:ty: $raw:ty, $from:expr, $to:expr;)*) => {$(
        impl Value for $type {
            const BYTES: usize = <$raw as Value>::BYTES;
            const ALIGN: usize = <$raw as Value>::ALIGN;
            fn load(memory: &impl Memory, at: Location<Self>) -> Self { $from(<$raw>::load(memory, at.retype())) }
            fn store(self, memory: &mut impl Memory, at: Location<Self>) { $to(self).store(memory, at.retype()) }
            fn decode(bytes: &[u8]) -> Self { $from(<$raw>::decode(bytes)) }
            fn encode(self, into: &mut [u8]) { $to(self).encode(into) }
        }
    )*};
}

/// Bits 0, 1 and 2 of the word, which the measurement tick tests in that order (0xf328b0,
/// 0xf328dc, 0xf328ec); no writer sets another bit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StatisticsInUse {
    pub primary: bool,
    pub search: bool,
    pub auxiliary: bool,
}

impl StatisticsInUse {
    const fn from_word(word: u32) -> Self { Self { primary: word & 1 != 0, search: word & 2 != 0, auxiliary: word & 4 != 0 } }

    const fn word(self) -> u32 { self.primary as u32 | (self.search as u32) << 1 | (self.auxiliary as u32) << 2 }
}

/// The guest address of the buffer measurement captures go to; the word is 0 while none is
/// attached.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureBuffer(NonZeroU32);

impl CaptureBuffer {
    pub const fn new(address: u32) -> Option<Self> {
        match NonZeroU32::new(address) {
            Some(address) => Some(Self(address)),
            None => None,
        }
    }

    pub const fn address(self) -> u32 { self.0.get() }

    const fn from_word(word: u32) -> Option<Self> { Self::new(word) }

    const fn word(buffer: Option<Self>) -> u32 {
        match buffer {
            Some(buffer) => buffer.address(),
            None => 0,
        }
    }
}

/// How the record's profile selects its measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MeasurementSelection {
    #[default]
    Unselected,
    Record,
    Other(u8),
}

impl MeasurementSelection {
    pub(crate) const fn from_byte(byte: u8) -> Self {
        match byte {
            0 => Self::Unselected,
            1 => Self::Record,
            other => Self::Other(other),
        }
    }

    const fn byte(self) -> u8 {
        match self {
            Self::Unselected => 0,
            Self::Record => 1,
            Self::Other(other) => other,
        }
    }
}

/// The measurement kinds the round is offered; 0 offers none.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct OfferedMeasurements(pub u32);

/// What the profile does with the frame-interval history.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct IntervalTracking(pub u8);

/// The status of the latest window calculation.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowStatus(pub u32);

/// The window position, low halfword first (0x758198..0x7581a4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WindowPosition(pub [u16; 2]);

impl WindowPosition {
    const fn from_word(word: u32) -> Self { Self([word as u16, (word >> 16) as u16]) }

    const fn word(self) -> u32 { self.0[0] as u32 | (self.0[1] as u32) << 16 }
}

const fn callback_word(callback: StockCallbackWord) -> u32 { callback.0 }

stored_as! {
    StatisticsInUse: u32, StatisticsInUse::from_word, StatisticsInUse::word;
    Option<CaptureBuffer>: u32, CaptureBuffer::from_word, CaptureBuffer::word;
    MeasurementSelection: u8, MeasurementSelection::from_byte, MeasurementSelection::byte;
    OfferedMeasurements: u32, OfferedMeasurements, |offered: OfferedMeasurements| offered.0;
    IntervalTracking: u8, IntervalTracking, |tracking: IntervalTracking| tracking.0;
    WindowStatus: u32, WindowStatus, |status: WindowStatus| status.0;
    WindowPosition: u32, WindowPosition::from_word, WindowPosition::word;
    StockCallbackWord: u32, StockCallbackWord, callback_word;
}
