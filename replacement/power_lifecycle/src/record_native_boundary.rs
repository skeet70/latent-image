//! Native C-ABI boundary over the context-first record bodies.
//!
//! The stock image calls `xm1_power_record_helper_<address>` entries with the
//! guest address space implicit in the CPU; a context-first body takes
//! `&mut RecordContext` instead.

use crate::record_address_space::{MemoryError, RecordError};
use crate::record_image_init_state::ImageInitError;

#[cfg(target_arch = "arm")]
use crate::record_address_space::{
    ByteCount, GuestAddress, GuestDomain, GuestStack, NativeAddressSpace, RecordContext,
    RegionDescriptor, RegionDomain, RegionPermission,
};

/// A body error the stock function cannot produce.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AbiFault;

impl From<MemoryError> for AbiFault {
    fn from(_: MemoryError) -> Self { Self }
}
impl From<RecordError> for AbiFault {
    fn from(_: RecordError) -> Self { Self }
}
impl From<ImageInitError> for AbiFault {
    fn from(_: ImageInitError) -> Self { Self }
}

pub fn optional_address(word: u32) -> Option<crate::record_address_space::GuestAddress> {
    if word == 0 { None } else { Some(crate::record_address_space::GuestAddress::new(word)) }
}

#[cfg(target_arch = "arm")]
extern "C" {
    static __power_cpu_start: u8;
    static __power_cpu_end: u8;
    fn xm1_power_dcache_clean_range(address: u32, bytes: u32);
    fn xm1_rtos_own_first_slot_1618() -> u32;
    fn xm1_rtos_own_first_slot_164() -> u32;
    fn xm1_rtos_own_first_slot_168(saved: u32) -> u32;
    fn xm1_rtos_global_lock_observe(word: u32) -> u32;
    fn xm1_rtos_global_lock_try(word: u32, stamp: u32) -> i32;
    fn xm1_rtos_global_lock_release(word: u32) -> i32;
}

#[cfg(target_arch = "arm")]
const GUEST_STACK_BYTES: usize = 4096;

#[cfg(target_arch = "arm")]
#[repr(align(8))]
struct GuestStackReservation([u8; GUEST_STACK_BYTES]);

/// Guest frames claimed by `RecordContext::with_guest_frame`.
///
/// Only its address is taken; no reference to the reservation is created while
/// a context borrows the address space.
#[cfg(target_arch = "arm")]
static mut GUEST_STACK: GuestStackReservation = GuestStackReservation([0; GUEST_STACK_BYTES]);

/// Halts the CPU.
#[cfg(target_arch = "arm")]
#[cold]
pub fn fault() -> ! {
    unsafe { core::arch::asm!("2: b 2b", options(noreturn, nostack)); }
}

/// Builds the one native record context and runs `body` with it.
///
/// The admitted table is the guest address space outside the replacement image
/// plus the guest stack reservation. `__power_cpu_start`/`__power_cpu_end` are
/// the composition's placement of this crate.
///
/// # Safety
///
/// Every admitted region is outside the replacement image, remains mapped for
/// the borrow, and is valid for volatile byte, halfword and word access. The
/// only Rust object a region covers is the guest stack reservation, which is
/// reached only through `GuestAddressSpace`.
#[cfg(target_arch = "arm")]
pub unsafe fn with_native_context<E: Default, R>(
    body: impl FnOnce(&mut RecordContext<NativeAddressSpace<'_>, E>) -> R,
) -> R {
    let start = unsafe { core::ptr::addr_of!(__power_cpu_start) as u32 };
    let end = unsafe { core::ptr::addr_of!(__power_cpu_end) as u32 };
    let stack = unsafe { core::ptr::addr_of!(GUEST_STACK) as u32 };
    let region = |base: u32, length: u32| match RegionDescriptor::new(
        GuestAddress::new(base),
        ByteCount(length),
        RegionDomain::Shared,
        RegionPermission::ReadWrite,
    ) {
        Ok(region) => region,
        Err(_) => fault(),
    };
    let before = region(0, start);
    let after = region(end, 0u32.wrapping_sub(end));
    let reservation = region(stack, GUEST_STACK_BYTES as u32);
    let placeholder = region(0, 1);
    let mut regions = [placeholder, placeholder, placeholder];
    let mut count = 0;
    if start != 0 { regions[count] = before; count += 1; }
    if end != 0 {
        regions[count] = after;
        count += 1;
        regions[count] = reservation;
        count += 1;
    }
    let memory = match NativeAddressSpace::new(GuestDomain::A, &regions[..count]) {
        Ok(memory) => memory,
        Err(_) => fault(),
    };
    let stack = match GuestStack::new(GuestAddress::new(stack), ByteCount(GUEST_STACK_BYTES as u32)) {
        Ok(stack) => stack,
        Err(_) => fault(),
    };
    let mut context = RecordContext::with_stack(memory, E::default(), stack);
    body(&mut context)
}

#[cfg(target_arch = "arm")]
#[derive(Default)]
pub struct NativeImageCache;

#[cfg(target_arch = "arm")]
impl crate::record_image_command_results::ImageCache for NativeImageCache {
    fn clean_cache(&mut self, address: GuestAddress, length: ByteCount) -> Result<(), crate::shutdown::KernelStatus> {
        unsafe { xm1_power_dcache_clean_range(address.get(), length.0) };
        Ok(())
    }
}

#[cfg(target_arch = "arm")]
#[derive(Default)]
pub struct NativeCachePublication;

#[cfg(target_arch = "arm")]
impl crate::record_scan_publication_workspace::CachePublication for NativeCachePublication {
    fn clean_data_cache(&mut self, address: GuestAddress, length: ByteCount) -> Result<(), MemoryError> {
        unsafe { xm1_power_dcache_clean_range(address.get(), length.0) };
        Ok(())
    }
}

/// Interrupt masking and the global lock through the RTOS primitives `osal::native` also uses.
#[cfg(target_arch = "arm")]
#[derive(Default)]
pub struct NativeCriticalKernel;

#[cfg(target_arch = "arm")]
impl crate::osal::CriticalKernel for NativeCriticalKernel {
    fn core(&self) -> crate::osal::CpuCore {
        match crate::osal::CpuCore::from_abi(unsafe { xm1_rtos_own_first_slot_1618() }) {
            Some(core) => core,
            None => fault(),
        }
    }
    fn mask_interrupts(&mut self) -> crate::osal::SavedInterrupts {
        crate::osal::SavedInterrupts(unsafe { xm1_rtos_own_first_slot_164() })
    }
    fn restore_interrupts(&mut self, saved: crate::osal::SavedInterrupts) {
        unsafe { let _ = xm1_rtos_own_first_slot_168(saved.0); }
    }
    fn observe_global_lock(&mut self) -> crate::osal::LockStamp {
        crate::osal::LockStamp(unsafe { xm1_rtos_global_lock_observe(0) })
    }
    fn try_global_lock(&mut self, stamp: crate::osal::LockStamp) -> bool {
        unsafe { xm1_rtos_global_lock_try(0, stamp.0) == 0 }
    }
    fn release_global_lock(&mut self) {
        unsafe { let _ = xm1_rtos_global_lock_release(0); }
    }
    fn nesting(&self) -> u32 {
        unsafe { core::ptr::read_volatile(0x40a0_f824 as *const u32) }
    }
    fn set_nesting(&mut self, depth: u32) {
        unsafe { core::ptr::write_volatile(0x40a0_f824 as *mut u32, depth); }
    }
}
