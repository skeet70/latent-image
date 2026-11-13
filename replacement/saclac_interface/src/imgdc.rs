#![forbid(unsafe_code)]

//! Payloads of the requests saclac posts to imgdc.
//!
//! Every field names the store that writes it: payload offset, width and the
//! instruction address. Bytes no store covers stay zero.

use crate::stock::{Encoded, StockCallbackWord};

/// Bytes a requester built and imgdc reads whole; its fields are the requester's to type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Record<const BYTES: usize>(pub [u8; BYTES]);

/// What imgdc reads for one request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImgDcMessage<'a> {
    Bytes(Encoded),
    /// The message carries no bytes; stock copies the record to `address` and imgdc reads it there.
    Block { address: u32, capacity: usize, record: &'a [u8] },
}

/// One request to imgdc; a variant with a callback names the word imgdc enters when it finishes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImgDcPayload {
    /// X-M1 1.50: 0xfea468 posts 0x3f16 with 16 bytes from its four arguments.
    Selection {
        /// Payload 0, 4 bytes, stored at 0xfea474.
        callback: StockCallbackWord,
        /// Payload 4, 4 bytes, stored at 0xfea478.
        first: u32,
        /// Payload 8, 4 bytes, stored at 0xfea47c.
        second: u32,
        /// Payload 12, 4 bytes, stored at 0xfea480.
        third: u32,
    },
    /// X-M1 1.50: 0xfea49c posts 0x3f26 with 232 bytes; 0xf31538 passes the record.
    Scan {
        /// Payload 0, 4 bytes, stored at 0xfea4a8.
        callback: StockCallbackWord,
        /// Payload 4..232, copied at 0xfea4b4.
        record: Record<228>,
    },
    /// X-M1 1.50: 0xfea4fc posts 0x3f28 with 4 bytes.
    Request28 {
        /// Payload 0, 4 bytes, stored at 0xfea50c.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0xfea524 posts 0x3f29 with 8 bytes.
    Request29 {
        /// Payload 0, 4 bytes, stored at 0xfea530.
        callback: StockCallbackWord,
        /// Payload 4, 4 bytes, stored at 0xfea534.
        value: u32,
    },
    /// X-M1 1.50: 0xfea550 posts 0x3f2a with 8 bytes.
    Request2a {
        /// Payload 0, 4 bytes, stored at 0xfea55c.
        callback: StockCallbackWord,
        /// Payload 4, 4 bytes, stored at 0xfea560.
        value: u32,
    },
    /// X-M1 1.50: 0xfea5c0 copies the 296-byte record to 0x016f4488 and posts 0x3f2c with no bytes.
    Request2c {
        /// The record, copied at 0xfea5d4.
        record: Record<296>,
    },
    /// X-M1 1.50: 0xfea5f0 posts 0x3f2d with 44 bytes copied from its argument.
    Request2d {
        /// Payload 0..44, copied at 0xfea604..0xfea618.
        record: Record<44>,
    },
    /// X-M1 1.50: 0xfea6ac posts 0x3f2f with no bytes.
    Request2f,
    /// X-M1 1.50: 0xfea868 posts 0x3f1b with no bytes.
    Request1b,
    /// X-M1 1.50: 0xfea98c posts 0x3f21 with no bytes.
    Request21,
    /// X-M1 1.50: 0xfea9a8 posts 0x3f22 with 4 bytes.
    Request22 {
        /// Payload 0, 4 bytes, stored at 0xfea9b8.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0xfea9d0 posts 0x3f23 with 4 bytes.
    Request23 {
        /// Payload 0, 4 bytes, stored at 0xfea9e0.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0xfeaa64 posts 0x3f42 with 136 bytes.
    ChildSelection {
        /// Payload 0..124, copied at 0xfeaa88.
        record: Record<124>,
        /// Payload 124, 4 bytes, stored at 0xfeaa70.
        callback: StockCallbackWord,
        /// Payload 128, 4 bytes, stored at 0xfeaa74; the caller passes the entry 0xf22928.
        first: StockCallbackWord,
        /// Payload 132, 4 bytes, stored at 0xfeaa78; the caller passes the entry 0xf22794.
        second: StockCallbackWord,
    },
    /// X-M1 1.50: 0xfeaaa4 posts 0x3f43 with 4 bytes.
    Request43 {
        /// Payload 0, 4 bytes, stored at 0xfeaab4.
        callback: StockCallbackWord,
    },
    /// X-M1 1.50: 0xfeaaec posts 0x3f44 with 24 bytes; payload 6..8 is unwritten.
    Request44 {
        /// Payload 0, 4 bytes, stored at 0xfeaaf8.
        callback: StockCallbackWord,
        /// Payload 4, 2 bytes, stored at 0xfeab00.
        halfword: u16,
        /// Payload 8, 4 bytes, stored at 0xfeab08.
        first: u32,
        /// Payload 12, 4 bytes, stored at 0xfeab10.
        second: u32,
        /// Payload 16, 4 bytes, stored at 0xfeab18.
        third: u32,
        /// Payload 20, 4 bytes, stored at 0xfeab20.
        fourth: u32,
    },
    /// X-M1 1.50: 0xfeab3c copies the 80-byte record to 0x016f224c and posts 0x3f45 with no bytes.
    Request45 {
        /// The record, copied at 0xfeab54.
        record: Record<80>,
    },
    /// X-M1 1.50: 0xfead98 posts 0x3f3a with 40 bytes; payload 9..12 is unwritten.
    Request3a {
        /// Payload 0, 4 bytes, stored at 0xfeada4.
        callback: StockCallbackWord,
        /// Payload 4, 4 bytes, stored at 0xfeada8.
        first: u32,
        /// Payload 8, 1 byte, stored at 0xfeadac.
        second: u8,
        /// Payload 12, 4 bytes, stored at 0xfeadb0.
        third: u32,
        /// Payload 16, 4 bytes, stored at 0xfeadb8.
        fourth: u32,
        /// Payload 20, 4 bytes, stored at 0xfeadc0.
        fifth: u32,
        /// Payload 24, 4 bytes, stored at 0xfeadc8.
        sixth: u32,
        /// Payload 28, 4 bytes, stored at 0xfeadd0.
        seventh: u32,
        /// Payload 32, 4 bytes, stored at 0xfeadd8.
        eighth: u32,
        /// Payload 36, 4 bytes, stored at 0xfeade0.
        ninth: u32,
    },
    /// X-M1 1.50: 0xfeadfc posts 0x3f3b with no bytes.
    Request3b,
}
