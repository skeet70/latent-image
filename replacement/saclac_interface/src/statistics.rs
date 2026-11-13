#![forbid(unsafe_code)]

//! Payloads of the requests saclac posts to statistics (queue 48).
//!
//! Every field names the store that writes it: payload offset, width and the
//! instruction address. Bytes no store covers stay zero.

use crate::stock::StockCallbackWord;

/// Bytes a requester built and statistics reads whole; its fields are the requester's to type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record<const BYTES: usize>(pub [u8; BYTES]);

/// One request to statistics; a variant with a callback names the word statistics enters when it finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatisticsPayload {
    /// X-M1 1.50: 0x103a09c posts 0x400b with 4 bytes from its argument.
    Request0b {
        /// Payload 0, 4 bytes, stored at 0x103a0a8.
        value: u32,
    },
    /// X-M1 1.50: 0x103a13c posts 0x400e with 32 bytes; 0xf3be7c builds the record.
    Request0e {
        /// Payload 0..28, copied at 0x103a164.
        record: Record<28>,
        /// Payload 28, 4 bytes, stored at 0x103a148.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0x103a200 posts 0x400f with 4 bytes from its argument.
    Request0f {
        /// Payload 0, 4 bytes, stored at 0x103a20c.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0x103b064 posts 0x4043 with no bytes when the word at 0x016f3afc is zero.
    Request43,
}
