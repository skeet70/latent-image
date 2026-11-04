#![no_std]
//! Owned arms of the X-M1 1.50 property getter 0x61e110 and setter 0x624694. The stubs in
//! `entry.S` run inside the stock frame and return through the stock shared exits.

mod property_table;
mod table;

use property_table::PROPERTIES;
use table::{BankOffset, Field, Kind, Save, Width};

/// Bytes per bank image at 0x96cc64.
const BANK_STRIDE: u32 = 0x1c2;

#[derive(Clone, Copy)]
struct PropertyId(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
struct BankIndex(u32);

enum ReadMode {
    Live,
    Default,
}

enum SetValue {
    Default,
    Value(u32),
}

/// Byte offset from the bank-image base 0x96cc64 or the previous-value base 0x89ec00.
#[derive(Clone, Copy)]
#[repr(transparent)]
struct ImageOffset(u32);

extern "C" {
    fn xm1_bank_load8(offset: ImageOffset) -> u32;
    fn xm1_bank_load16(offset: ImageOffset) -> u32;
    fn xm1_bank_store8(offset: ImageOffset, value: u32);
    fn xm1_bank_store16(offset: ImageOffset, value: u32);
    fn xm1_save_load8(offset: ImageOffset) -> u32;
    fn xm1_save_load16(offset: ImageOffset) -> u32;
    fn xm1_save_store8(offset: ImageOffset, value: u32);
    fn xm1_save_store16(offset: ImageOffset, value: u32);
}

#[inline(always)]
fn entry(id: PropertyId) -> Kind {
    match PROPERTIES.get(id.0.wrapping_sub(1) as usize) {
        Some(entry) => entry.kind(),
        None => Kind::Delegated,
    }
}

fn bank_offset(bank: BankIndex, offset: BankOffset) -> ImageOffset {
    ImageOffset(bank.0.wrapping_mul(BANK_STRIDE).wrapping_add(offset.0))
}

unsafe fn bank_load(offset: ImageOffset, width: Width) -> u32 {
    match width {
        Width::Byte => xm1_bank_load8(offset),
        Width::Half => xm1_bank_load16(offset),
    }
}

unsafe fn bank_store(offset: ImageOffset, width: Width, value: u32) {
    match width {
        Width::Byte => xm1_bank_store8(offset, value),
        Width::Half => xm1_bank_store16(offset, value),
    }
}

unsafe fn save_load(offset: ImageOffset, width: Width) -> u32 {
    match width {
        Width::Byte => xm1_save_load8(offset),
        Width::Half => xm1_save_load16(offset),
    }
}

unsafe fn save_store(offset: ImageOffset, width: Width, value: u32) {
    match width {
        Width::Byte => xm1_save_store8(offset, value),
        Width::Half => xm1_save_store16(offset, value),
    }
}

/// `container` with the field's bits replaced by the low bits of `value`, as BFI does.
fn insert(field: Field, container: u32, value: u32) -> u32 {
    let mask = field.mask() << field.lsb;
    (container & !mask) | ((value << field.lsb) & mask)
}

fn extract(field: Field, container: u32) -> u32 {
    (container >> field.lsb) & field.mask()
}

unsafe fn get(id: PropertyId, bank: BankIndex, mode: ReadMode) -> u32 {
    match entry(id) {
        Kind::Field(field) => match mode {
            ReadMode::Default => field.default.0,
            ReadMode::Live => extract(field, bank_load(bank_offset(bank, field.offset), field.width)),
        },
        Kind::Unassigned | Kind::Delegated => 0,
    }
}

unsafe fn set(id: PropertyId, value: SetValue, bank: BankIndex) -> u32 {
    let field = match entry(id) {
        Kind::Field(field) => field,
        Kind::Unassigned | Kind::Delegated => return 0,
    };
    let stored = match value {
        SetValue::Default => field.default.0,
        SetValue::Value(value) => value,
    };
    if bank == BankIndex(0) && field.save == Save::Bank0Previous {
        let at = bank_offset(BankIndex(0), field.offset);
        let previous = bank_load(at, field.width);
        let saved = if field.whole() { previous } else { insert(field, save_load(at, field.width), previous >> field.lsb) };
        save_store(at, field.width, saved);
    }
    let at = bank_offset(bank, field.offset);
    let container = if field.whole() { stored } else { insert(field, bank_load(at, field.width), stored) };
    bank_store(at, field.width, container);
    1
}

/// Getter arm body: `id` is the stock r12, `bank` r1 and `defaults` r2 at the arm.
///
/// # Safety
/// The bank image for `bank` must be mapped at 0x96cc64 + 0x1c2 * bank.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_get(id: u32, bank: u32, defaults: u32) -> u32 {
    get(PropertyId(id), BankIndex(bank), if defaults == 1 { ReadMode::Default } else { ReadMode::Live })
}

/// Setter arm body: `id`, `value`, `bank` and `default_requested` are the stock r4, r5, r6 and r7 at the arm.
///
/// # Safety
/// The bank image for `bank` must be mapped at 0x96cc64 + 0x1c2 * bank, and for bank 0 the
/// previous-value image at 0x89ec00.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_set(id: u32, value: u32, bank: u32, default_requested: u32) -> u32 {
    set(PropertyId(id), if default_requested != 0 { SetValue::Default } else { SetValue::Value(value) }, BankIndex(bank))
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
