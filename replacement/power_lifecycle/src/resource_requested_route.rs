//! X-M1 1.50 global requested output level at 0x0064982c.


#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestedSlot(u8);
impl RequestedSlot {
    pub const SERVICE_RESULT: Self = Self(2);
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value <= 18 { Some(Self(value as u8)) } else { None }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestedLevel(u8);
impl RequestedLevel {
    pub const SERVICE_RESULT: Self = Self(8);
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value <= 8 { Some(Self(value as u8)) } else { None }
    }
}

#[cfg(target_arch = "arm")]
pub use native::{xm1_power_output_route_set_requested_level, NativeRequestedRoute};
#[cfg(target_arch = "arm")]
mod native {
use super::*;
use core::ptr::{read_volatile, write_volatile};
use crate::resource_route::{self, RequestLevel, RequestSlot, ResourceSelector, RouteError};

const ROUTES: usize = 0x409c1e30;

unsafe extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
}

pub struct NativeRequestedRoute { _mapped: () }
impl NativeRequestedRoute {
    /// # Safety
    /// The shared output route constructor must have completed and its state, source table and device registers must remain mapped.
    pub const unsafe fn new() -> Self { Self { _mapped: () } }

    pub fn request(&mut self, slot: RequestedSlot, level: RequestedLevel) {
        unsafe {
            let saved = xm1_power_interrupt_lock();
            write_volatile((ROUTES + 8 + slot.0 as usize * 4) as *mut u32, level.0 as u32);
            let mut resolved = read_volatile((ROUTES + 4) as *const u32);
            for index in 0..19 {
                let requested = read_volatile((ROUTES + 8 + index * 4) as *const u32);
                resolved = resolved.max(requested);
            }
            resolved = resolved.min(read_volatile(ROUTES as *const u32));
            // The stock caller ignores both channel application results.
            let level = RequestLevel::from_abi(resolved);
            const SLOT: RequestSlot = match RequestSlot::from_abi(0) {
                Some(slot) => slot,
                None => unreachable!(),
            };
            const FIRST: ResourceSelector = match ResourceSelector::from_abi(0) {
                Ok(selector) => selector,
                Err(_) => unreachable!(),
            };
            const SECOND: ResourceSelector = match ResourceSelector::from_abi(0x101) {
                Ok(selector) => selector,
                Err(_) => unreachable!(),
            };
            if let Some(level) = level {
                let _ = resource_route::set_request(FIRST, SLOT, level);
                let _ = resource_route::set_request(SECOND, SLOT, level);
            }
            xm1_power_interrupt_unlock(saved);
        }
    }
}

/// # Safety
/// The shared output route constructor must have completed and its state, source table and device registers must remain mapped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_output_route_set_requested_level(slot: u32, level: u32, _unused: u32) -> i32 {
    match (RequestedSlot::from_abi(slot), RequestedLevel::from_abi(level)) {
        (Some(slot), Some(level)) => {
            unsafe { NativeRequestedRoute::new() }.request(slot, level);
            0
        }
        _ => RouteError::InvalidLevel.status(),
    }
}
}
