//! The playback request block at 0x014ea6e4 keeps its stock layout while stock builds, copies and
//! reads it by offset: the queue-4 receive writes the message code halfword at +0 and copies the
//! payload to 0x014ea6f0. Owned machines name what they read through these views.

pub const BLOCK: u32 = 0x014e_a6e4;
pub const PAYLOAD: u32 = BLOCK + 0xc;

/// Payload words of message 0x119, by offset: the words its stock builder (image 5 0x00db2fc0)
/// stores, named by the builder argument word each copies. The other 148 of its 196 payload bytes
/// are unspecified.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum Message119 {
    Argument18 = 0x00,
    Argument1c = 0x04,
    Argument14 = 0x18,
    Argument10 = 0x1c,
    /// Argument word 0; machine 7 routes on it.
    Event = 0x20,
    Argument04 = 0x24,
    Argument0c = 0x2c,
    /// The builder's own 0x119.
    Code = 0x48,
    Argument08 = 0x58,
    Argument20 = 0x60,
    Argument2c = 0xbc,
    Argument30 = 0xc0,
}

impl Message119 {
    pub const fn address(self) -> u32 {
        PAYLOAD + self as u32
    }
}
