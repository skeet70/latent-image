//! A32 stage adapter: the stock `+0x0`/`+0x4` entries over the typed record core.
#![cfg_attr(not(test), no_std)]

#[path = "../src/lib.rs"]
mod records;

use core::arch::global_asm;
use core::ptr::{read_volatile, write_volatile};
use records::layout::{Domain, GuestAddress, TaskSlot};
use records::{create_masked_at, delete_masked, DeleteMode, Machine, SavedInterrupts, TaskRecordError};

#[cfg(feature = "domain-a")]
const ENTRY: Domain = Domain::A;
#[cfg(feature = "domain-b")]
const ENTRY: Domain = Domain::B;

extern "C" {
    fn xm1_task_mask_save() -> u32;
    fn xm1_task_mask_restore(saved: u32);
}

#[inline(always)]
fn load(at: GuestAddress) -> u32 {
    unsafe { read_volatile(at.0 as *const u32) }
}

#[inline(always)]
fn store(at: GuestAddress, value: u32) {
    unsafe { write_volatile(at.0 as *mut u32, value) }
}

#[inline(always)]
fn load_byte(at: GuestAddress) -> u8 {
    unsafe { read_volatile(at.0 as *const u8) }
}

#[inline(always)]
fn store_byte(at: GuestAddress, value: u8) {
    unsafe { write_volatile(at.0 as *mut u8, value) }
}

struct Cpu;

impl Machine for Cpu {
    fn load_word(&self, at: GuestAddress) -> u32 {
        load(at)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        store(at, value)
    }

    fn load_byte(&self, at: GuestAddress) -> u8 {
        load_byte(at)
    }

    fn store_byte(&self, at: GuestAddress, value: u8) {
        store_byte(at, value)
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        SavedInterrupts(unsafe { xm1_task_mask_save() })
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        unsafe { xm1_task_mask_restore(saved.0) }
    }

    fn dmb(&self) {
        unsafe { core::arch::asm!("dmb sy", options(nostack, preserves_flags)) };
    }
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.task_create_body"]
pub extern "C" fn xm1_rtos_task_create_body(id: u32, request_at: u32) -> i32 {
    let saved = Cpu.mask_interrupts();
    let result = TaskSlot::new(id & 0xffff)
        .ok_or(TaskRecordError::InvalidSlot)
        .and_then(|slot| create_masked_at(&Cpu, ENTRY, slot, GuestAddress(request_at)));
    let status = result.map_or_else(TaskRecordError::status, |_| 0);
    Cpu.restore_interrupts(saved);
    status
}

#[no_mangle]
#[inline(never)]
#[link_section = ".text.task_delete_body"]
pub extern "C" fn xm1_rtos_task_delete_body(id: u32, mode: u32) -> i32 {
    let saved = Cpu.mask_interrupts();
    let mode = if mode == 1 { DeleteMode::Reset } else { DeleteMode::Clear };
    let result = TaskSlot::new(id & 0xffff)
        .ok_or(TaskRecordError::InvalidSlot)
        .and_then(|slot| delete_masked(&Cpu, ENTRY, slot, mode));
    let status = result.map_or_else(TaskRecordError::status, |_| 0);
    Cpu.restore_interrupts(saved);
    status
}

global_asm!(
    ".pushsection .text.task_create_entry, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_task_create",
    ".type xm1_rtos_task_create, %function",
    "xm1_rtos_task_create:",
    "b xm1_rtos_task_create_body",
    ".size xm1_rtos_task_create, . - xm1_rtos_task_create",
    ".popsection",
    ".pushsection .text.task_delete_entry, \"ax\", %progbits",
    ".arm",
    ".global xm1_rtos_task_delete",
    ".type xm1_rtos_task_delete, %function",
    "xm1_rtos_task_delete:",
    "b xm1_rtos_task_delete_body",
    ".size xm1_rtos_task_delete, . - xm1_rtos_task_delete",
    ".popsection",
);

#[cfg(all(target_arch = "arm", not(test), not(feature = "component")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
