const ROUTES: usize = 0x409c1e30;
const SOURCES: usize = 0x7a20d4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ResourceSelector(u16);
impl ResourceSelector {
    pub const fn from_abi(value: u32) -> Result<Self, RouteError> {
        if value as u8 <= 18 { Ok(Self(value as u16)) } else { Err(RouteError::InvalidSelector) }
    }
    pub const fn as_abi(self) -> u32 { self.0 as u32 }
    const fn channel(self) -> RouteChannel { RouteChannel((self.0 >> 8) as u8) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RequestSlot(u8);
impl RequestSlot {
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value <= 36 { Some(Self(value as u8)) } else { None }
    }
    pub const fn as_abi(self) -> u32 { self.0 as u32 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RequestLevel(u8);
impl RequestLevel {
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value <= 11 { Some(Self(value as u8)) } else { None }
    }
    pub const fn as_abi(self) -> u32 { self.0 as u32 }
}

#[derive(Clone, Copy)]
struct RouteChannel(u8);
impl RouteChannel { const fn state(self) -> usize { ROUTES + self.0 as usize * 160 } }

#[derive(Clone, Copy)]
struct ResolvedLevel(u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RouteError {
    InvalidSelector,
    InvalidRequestSlot,
    InvalidLevel,
    UnsupportedSource,
    UnsupportedChannel,
}
impl RouteError { pub const fn status(self) -> i32 { -30 } }

#[cfg(target_arch = "arm")]
pub use native::{set_request, xm1_power_resource_route_request};
#[cfg(target_arch = "arm")]
mod native {
use super::*;
use core::ptr::{read_volatile, write_volatile};

unsafe extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
    fn xm1_power_save_irq() -> u32;
    fn xm1_power_restore_irq(saved: u32);
}

unsafe fn word(address: usize) -> u32 { unsafe { read_volatile(address as *const u32) } }
unsafe fn put(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value) } }

unsafe fn resolve(channel: RouteChannel) -> ResolvedLevel {
    unsafe {
        let state = channel.state();
        let mut level = word(state + 0x58);
        for request in 0..37 {
            let requested = word(state + 0x5c + request * 4);
            if requested > level { level = requested; }
        }
        let maximum = word(state + 0x54);
        if maximum < level { level = maximum; }
        ResolvedLevel(level)
    }
}

unsafe fn apply(channel: RouteChannel, level: ResolvedLevel) -> Result<(), RouteError> {
    unsafe {
        let state = channel.state();
        let source = word(SOURCES + (channel.0 as usize * 12 + level.0 as usize) * 4);
        if source == u32::MAX { return Err(RouteError::UnsupportedSource); }
        if word(state + 0xf0) != level.0 {
            let (register, preserved, shift) = match channel.0 {
                0..=8 => (0xfff88000 + channel.0 as usize * 4, !7u32, 0),
                9 => (0xfff88024, 0x8fffffff, 28),
                _ => return Err(RouteError::UnsupportedChannel),
            };
            let irq = xm1_power_save_irq();
            put(register, (word(register) & preserved) | (source << shift));
            xm1_power_restore_irq(irq);
        }
        put(state + 0xf0, level.0);
        Ok(())
    }
}

unsafe fn set_locked(selector: ResourceSelector, slot: RequestSlot, level: RequestLevel) -> Result<(), RouteError> {
    unsafe {
        let channel = selector.channel();
        put(channel.state() + 0x5c + slot.0 as usize * 4, level.as_abi());
        apply(channel, resolve(channel))
    }
}

/// # Safety
/// The startup route constructor must have completed. Native route state, source table, and selected device registers must be mapped.
pub unsafe fn set_request(selector: ResourceSelector, slot: RequestSlot, level: RequestLevel) -> Result<(), RouteError> {
    unsafe {
        let saved = xm1_power_interrupt_lock();
        let result = set_locked(selector, slot, level);
        xm1_power_interrupt_unlock(saved);
        result
    }
}

/// # Safety
/// Native route state, source table, and selected device registers must be mapped for the selector's channel byte.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_resource_route_request(selector: u32, slot: u32, level: u32) -> i32 {
    unsafe {
        let saved = xm1_power_interrupt_lock();
        let result = match (RequestSlot::from_abi(slot), RequestLevel::from_abi(level), ResourceSelector::from_abi(selector)) {
            (None, _, _) => Err(RouteError::InvalidRequestSlot),
            (_, None, _) => Err(RouteError::InvalidLevel),
            (_, _, Err(error)) => Err(error),
            (Some(slot), Some(level), Ok(selector)) => set_locked(selector, slot, level),
        };
        xm1_power_interrupt_unlock(saved);
        match result { Ok(()) => 0, Err(error) => error.status() }
    }
}
}
