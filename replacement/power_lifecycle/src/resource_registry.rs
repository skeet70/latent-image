use core::num::NonZeroU32;

const ROUTES: usize = 0x409c1e30;
const ACTIVE: usize = 0x409c24cc;
const HANDLES: usize = 0x409c2588;
const SERIAL: usize = 0x409c2804;
const CLOCKS: usize = 0x7c86d4;
const RESOURCE_COUNT: usize = 47;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct ResourceId(u8);
impl ResourceId {
    pub const fn from_abi(value: u32) -> Option<Self> {
        if value < RESOURCE_COUNT as u32 { Some(Self(value as u8)) } else { None }
    }
    pub const fn as_abi(self) -> u32 { self.0 as u32 }
}

#[derive(Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct RegistrationHandle(NonZeroU32);
impl RegistrationHandle {
    pub const fn from_abi(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn as_abi(&self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistryError { InvalidResource, AlreadyReserved, UnknownHandle }
impl RegistryError {
    pub const fn status(self) -> i32 {
        match self { Self::InvalidResource => -30, Self::AlreadyReserved | Self::UnknownHandle => -20 }
    }
}

#[cfg(target_arch = "arm")]
pub use native::{acquire, release, xm1_power_resource_acquire, xm1_power_resource_initialize_routes, xm1_power_resource_release};
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

unsafe fn acquire_into(resource: u32, output: *mut u32) -> Result<(), RegistryError> {
    unsafe {
        let saved = xm1_power_interrupt_lock();
        let result = match ResourceId::from_abi(resource) {
            None => Err(RegistryError::InvalidResource),
            Some(resource) => {
                let index = resource.0 as usize;
                if word(HANDLES + index * 4) != 0 {
                    Err(RegistryError::AlreadyReserved)
                } else {
                    loop {
                        put(SERIAL, word(SERIAL).wrapping_add(1));
                        if word(SERIAL) == 0 { put(SERIAL, word(SERIAL).wrapping_add(1)); }
                        let mut collision = false;
                        for other in 0..RESOURCE_COUNT {
                            if word(HANDLES + other * 4) == word(SERIAL) {
                                collision = true;
                                break;
                            }
                        }
                        if !collision { break; }
                    }
                    put(HANDLES + index * 4, word(SERIAL));
                    write_volatile(output, word(HANDLES + index * 4));
                    put(ACTIVE + index * 4, 1);
                    let mask = word(CLOCKS + index * 8 + 4);
                    let bank = word(CLOCKS + index * 8);
                    let irq = xm1_power_save_irq();
                    let register = if bank == 0 { 0xfff8802c } else { 0xfff88030 };
                    put(register, (word(register) & !mask) | mask);
                    xm1_power_restore_irq(irq);
                    Ok(())
                }
            }
        };
        xm1_power_interrupt_unlock(saved);
        result
    }
}

/// # Safety
/// The route constructor must have completed, and native registry RAM and clock registers must be mapped.
pub unsafe fn acquire(resource: ResourceId) -> Result<RegistrationHandle, RegistryError> {
    let mut output = 0;
    unsafe { acquire_into(resource.as_abi(), &mut output)?; }
    Ok(RegistrationHandle(unsafe { NonZeroU32::new_unchecked(output) }))
}

unsafe fn release_value(handle: u32) -> Result<(), RegistryError> {
    unsafe {
        let saved = xm1_power_interrupt_lock();
        let mut selected = None;
        if handle != 0 {
            for index in 0..RESOURCE_COUNT {
                if word(HANDLES + index * 4) == handle {
                    selected = Some(index);
                    break;
                }
            }
        }
        let result = if let Some(index) = selected {
            put(ACTIVE + index * 4, 0);
            let bank = word(CLOCKS + index * 8);
            let mut retained = 0;
            for other in 0..RESOURCE_COUNT {
                if word(ACTIVE + other * 4) == 1 && word(CLOCKS + other * 8) == bank {
                    retained |= word(CLOCKS + other * 8 + 4);
                }
            }
            let removed = word(CLOCKS + index * 8 + 4) & !retained;
            if word(0x4089e810) != 0x19790515 {
                let irq = xm1_power_save_irq();
                let register = if bank == 0 { 0xfff8802c } else { 0xfff88030 };
                put(register, word(register) & !removed);
                xm1_power_restore_irq(irq);
            }
            put(HANDLES + index * 4, 0);
            Ok(())
        } else { Err(RegistryError::UnknownHandle) };
        xm1_power_interrupt_unlock(saved);
        result
    }
}

/// # Safety
/// The handle must belong to the current route-constructor lifetime; native registry RAM and clock registers must be mapped.
pub unsafe fn release(handle: RegistrationHandle) -> Result<(), RegistryError> {
    unsafe { release_value(handle.as_abi()) }
}

/// # Safety
/// `output` must be writable and aligned when `resource` is free and in range; native registry memory must be initialized and mapped.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_resource_acquire(resource: u32, output: *mut u32) -> i32 {
    match unsafe { acquire_into(resource, output) } { Ok(()) => 0, Err(error) => error.status() }
}

/// # Safety
/// Native registry RAM and clock registers must be mapped; `handle` must not refer to an earlier constructor lifetime.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_resource_release(handle: u32) -> i32 {
    match unsafe { release_value(handle) } { Ok(()) => 0, Err(error) => error.status() }
}

/// # Safety
/// This is the startup route constructor. No route requests or resource registrations may be live.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_power_resource_initialize_routes() {
    unsafe {
        let saved = xm1_power_interrupt_lock();
        for index in 0..0x205 { put(ROUTES + index * 4, 0); }
        for (channel, maximum) in [8, 8, 8, 9, 8, 10, 8, 8, 8, 11].into_iter().enumerate() {
            put(ROUTES + channel * 160 + 0x54, maximum);
            put(ROUTES + channel * 160 + 0x58, 0);
        }
        put(ROUTES, 8);
        put(ROUTES + 4, 0);
        for channel in [1, 2, 8] { put(ROUTES + channel * 160 + 0x50, 8); }
        xm1_power_interrupt_unlock(saved);
    }
}
}
