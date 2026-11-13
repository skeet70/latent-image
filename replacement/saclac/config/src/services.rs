#![forbid(unsafe_code)]

//! Small configuration-interface services task 43 shares: guest address aliases, the adjusted
//! tick delay, the lens status bit and lens requests by index (image 4, 0x740e0c..0x744eac).

/// A guest address with bit 30 cleared, the cached view of an uncached alias (0x740e0c).
pub const fn cached(address: u32) -> u32 { address & !(1 << 30) }

/// The ticks 0x743ce4 delays for: one more for a multiple of ten, bit 31 cleared.
pub const fn adjusted_delay(ticks: u32) -> u32 {
    let ticks = if ticks % 10 == 0 { ticks.wrapping_add(1) } else { ticks };
    ticks & !(1 << 31)
}

/// Whether the lens status byte 0x6c6b08 reads has bit 6 set (0x744eac).
pub const fn lens_status_bit(status: u8) -> bool { status & 0x40 != 0 }

/// The lenscom request number each request index selects (the table at 0x818554).
const REQUEST_NUMBERS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15];

/// The status 0x742a6c returns for an index above 14.
pub const REFUSED_INDEX: u32 = 0x84;
/// The status for a request lenscom did not take.
pub const NOT_POSTED: u32 = 0x81;

/// The lens request entry 0x742a6c enters (0x6b7750 forwarding to 0x6b731c), and the word it
/// stores the callback in first (0x96bd04).
pub trait LensRequests {
    fn store_callback(&mut self, callback: u32);
    /// 0x6b731c's result: nonzero when lenscom did not take the request.
    fn request(&mut self, number: u8, callback: u32) -> u32;
}

/// Stores `callback`, then requests the number `index` selects; 0 when posted (0x742a6c).
pub fn lens_request(requests: &mut impl LensRequests, index: u32, callback: u32) -> u32 {
    requests.store_callback(callback);
    let Some(&number) = REQUEST_NUMBERS.get(index as usize) else { return REFUSED_INDEX };
    if requests.request(number, callback) == 0 { 0 } else { NOT_POSTED }
}
