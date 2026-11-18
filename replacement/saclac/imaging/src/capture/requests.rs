#![forbid(unsafe_code)]

//! Decisions a capture request makes from its own words.

/// The request ids that always take the request's own enable bit.
const SELF_ENABLED: [u32; 2] = [0x31, 0x32];
const DISABLED_MODE: u32 = 3;

const fn enable_bit(flags: u32) -> bool { flags & 0b10 != 0 }

/// 0xfea3a0.
pub fn request_enabled(id: u32, flags: u32, mode: u32, fallback: u32) -> bool {
    let enabled = if enable_bit(fallback) || SELF_ENABLED.contains(&id) { enable_bit(flags) } else { true };
    enabled && mode != DISABLED_MODE
}
