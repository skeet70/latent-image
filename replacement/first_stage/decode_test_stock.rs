//! Stock's shared-engine reservation: `hw_fff430_acquire` 0x00683830, and the three
//! bookkeeping calls `hw_fff430_release` 0x006838dc makes after its enable write.

use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn xm1_decode_test_acquire();
    fn xm1_decode_test_release_clock(handle: u32);
    fn xm1_decode_test_clear_route(selector: u32, slot: u32, level: u32);
    fn xm1_decode_test_signal_resource(resource: u32);
}

/// The pair's eight-byte state block: word 0 holds the clock handle, word 4 the acquired flag.
const HANDLE: *mut u32 = 0x0089_f748 as *mut u32;
const RESOURCE_STATE: *const u32 = 0x0093_e79c as *const u32;
/// Channel 4 slot 19: 0x409c1e30 + 4 * (40 * 4 + 0x13 + 22) + 4 (0x00649a3c-0x00649a78).
const ROUTE_REQUEST: *const u32 = 0x409c_2158 as *const u32;

/// Clock index 0x29's handle, as 0x00649c4c stores it in word 0.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockHandle(u32);

impl ClockHandle {
    pub fn word(self) -> u32 {
        self.0
    }
}

pub struct Reservation {
    handle: ClockHandle,
}

pub struct ReservationTimeout;

/// Reserved resource 0x2b, route stage 0x410 line 0x13 and clock handle 0x29.
pub fn acquire() -> Result<Reservation, ReservationTimeout> {
    let before = handle();
    unsafe { xm1_decode_test_acquire() };
    let after = handle();
    match after {
        Some(handle) if after != before => Ok(Reservation { handle }),
        _ => Err(ReservationTimeout),
    }
}

pub fn handle() -> Option<ClockHandle> {
    match unsafe { read_volatile(HANDLE) } {
        0 => None,
        word => Some(ClockHandle(word)),
    }
}

/// 0x00649ddc, then word 0 = 0 (0x00683900-0x00683934).
pub fn release_clock(reservation: Reservation) {
    unsafe { xm1_decode_test_release_clock(reservation.handle.word()) };
    unsafe { write_volatile(HANDLE, 0) };
}

/// 0x006499d0's call at 0x00683938-0x00683944.
pub fn clear_route() {
    unsafe { xm1_decode_test_clear_route(0x410, 0x13, 0) };
}

/// 0x00724c00's call at 0x00683948-0x0068394c.
pub fn signal_resource() {
    unsafe { xm1_decode_test_signal_resource(0x2b) };
}

/// 0x0093e6f4 + (0x2b - 1) * 4, the word 0x00724b68 and 0x00724c00 pack into their arguments;
/// neither writes it.
pub fn resource_state() -> u32 {
    unsafe { read_volatile(RESOURCE_STATE) }
}

pub fn route_request() -> u32 {
    unsafe { read_volatile(ROUTE_REQUEST) }
}
