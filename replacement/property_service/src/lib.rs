#![no_std]
//! Owned arms of the X-M1 1.50 property getter 0x61e110 and setter 0x624694. The stubs in
//! `entry.S` run inside the stock frame and return through the stock shared exits.

mod property_table;
mod table;

use property_table::{PROPERTIES, SLOTS};
use table::{BankDefault, BankOffset, Bits, Kind, RecordOffset, Save, SlotIndex, SlotRecord, Width};

/// Bytes per bank image at 0x96cc64.
const BANK_STRIDE: u32 = 0x1c2;

/// Offset of slot record 0 within a bank image and the previous-value image.
const RECORD_BASE: u32 = 0x140;

/// Bytes per slot record, in the bank images and the slot defaults at 0x7cb804.
const RECORD_STRIDE: u32 = 0x28;

/// The setter value the stock prologue turns into a default request.
const DEFAULT_REQUEST: u32 = 0x1affffff;

#[derive(Clone, Copy)]
struct PropertyId(u32);

#[derive(Clone, Copy, PartialEq, Eq)]
struct BankIndex(u32);

/// The getter's and setter's argument 3; 1 selects the bank branch of a slot field.
#[derive(Clone, Copy, PartialEq, Eq)]
struct SlotNumber(u32);

const BANK_BRANCH: SlotNumber = SlotNumber(1);

enum ReadMode {
    Live,
    Default,
}

enum SetValue {
    Default,
    Value(u32),
}

/// Byte offset from the base of the image an accessor reads or writes.
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
    fn xm1_slot_defaults_load8(offset: ImageOffset) -> u32;
    fn xm1_slot_defaults_load16(offset: ImageOffset) -> u32;
    fn xm1_tail_defaults_load8(offset: ImageOffset) -> u32;
}

#[inline(always)]
fn entry(id: PropertyId) -> Kind {
    match PROPERTIES.get(id.0.wrapping_sub(1) as usize) {
        Some(entry) => entry.kind(),
        None => Kind::Delegated,
    }
}

#[inline(always)]
fn slot_record(index: SlotIndex) -> Option<SlotRecord> {
    match SLOTS.get(index.0 as usize) {
        Some(slot) => Some(slot.record()),
        None => None,
    }
}

fn bank_offset(bank: BankIndex, offset: BankOffset) -> ImageOffset {
    ImageOffset(bank.0.wrapping_mul(BANK_STRIDE).wrapping_add(offset.0))
}

/// Offset of a slot's record field from a bank-0-layout image base, without the bank term.
fn record_offset(slot: SlotNumber, offset: RecordOffset) -> u32 {
    RECORD_BASE.wrapping_add(slot.0.wrapping_mul(RECORD_STRIDE)).wrapping_add(offset.0)
}

fn bank_record_offset(bank: BankIndex, slot: SlotNumber, offset: RecordOffset) -> ImageOffset {
    ImageOffset(bank.0.wrapping_mul(BANK_STRIDE).wrapping_add(record_offset(slot, offset)))
}

fn slot_defaults_offset(slot: SlotNumber, offset: RecordOffset) -> ImageOffset {
    ImageOffset(slot.0.wrapping_mul(RECORD_STRIDE).wrapping_add(offset.0))
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

unsafe fn slot_defaults_load(offset: ImageOffset, width: Width) -> u32 {
    match width {
        Width::Byte => xm1_slot_defaults_load8(offset),
        Width::Half => xm1_slot_defaults_load16(offset),
    }
}

unsafe fn bank_default(default: BankDefault) -> u32 {
    match default {
        BankDefault::Constant(value) => value.0,
        BankDefault::TailByte(offset) => xm1_tail_defaults_load8(ImageOffset(offset.0)),
    }
}

/// `container` with the field's bits replaced by the low bits of `value`, as BFI does.
fn insert(bits: Bits, container: u32, value: u32) -> u32 {
    let mask = bits.mask() << bits.lsb;
    (container & !mask) | ((value << bits.lsb) & mask)
}

fn extract(bits: Bits, container: u32) -> u32 {
    (container >> bits.lsb) & bits.mask()
}

/// The setter's save and store of one field: `live` is its offset in `bank`'s image and `saved` its
/// offset in the previous-value image, which is also its offset in bank 0.
unsafe fn write(bits: Bits, save: Save, bank: BankIndex, live: ImageOffset, saved: ImageOffset, stored: u32) {
    if bank == BankIndex(0) && save == Save::Bank0Previous {
        let previous = bank_load(live, bits.width);
        let copied = if bits.whole() { previous } else { insert(bits, save_load(saved, bits.width), previous >> bits.lsb) };
        save_store(saved, bits.width, copied);
    }
    let container = if bits.whole() { stored } else { insert(bits, bank_load(live, bits.width), stored) };
    bank_store(live, bits.width, container);
}

unsafe fn get(id: PropertyId, bank: BankIndex, mode: ReadMode, slot: SlotNumber) -> u32 {
    match entry(id) {
        Kind::Field(field) => match mode {
            ReadMode::Default => field.default.0,
            ReadMode::Live => extract(field.bits, bank_load(bank_offset(bank, field.offset), field.bits.width)),
        },
        Kind::SlotField(slot_field) => {
            let Some(record) = slot_record(slot_field.index) else { return 0 };
            match (slot == BANK_BRANCH, mode) {
                (true, ReadMode::Default) => bank_default(record.bank_default),
                (true, ReadMode::Live) => extract(slot_field.bits, bank_load(bank_offset(bank, slot_field.offset), slot_field.bits.width)),
                (false, ReadMode::Default) => extract(record.bits, slot_defaults_load(slot_defaults_offset(slot, record.offset), record.bits.width)),
                (false, ReadMode::Live) => extract(record.bits, bank_load(bank_record_offset(bank, slot, record.offset), record.bits.width)),
            }
        }
        Kind::Unassigned | Kind::Delegated => 0,
    }
}

unsafe fn set(id: PropertyId, value: SetValue, bank: BankIndex, slot: SlotNumber) -> u32 {
    match entry(id) {
        Kind::Field(field) => {
            let stored = match value {
                SetValue::Default => field.default.0,
                SetValue::Value(value) => value,
            };
            write(field.bits, field.save, bank, bank_offset(bank, field.offset), bank_offset(BankIndex(0), field.offset), stored);
            1
        }
        Kind::SlotField(slot_field) => {
            let Some(record) = slot_record(slot_field.index) else { return 0 };
            if slot == BANK_BRANCH {
                let stored = match value {
                    SetValue::Default => bank_default(record.bank_default),
                    SetValue::Value(value) => value,
                };
                write(slot_field.bits, record.save, bank, bank_offset(bank, slot_field.offset), bank_offset(BankIndex(0), slot_field.offset), stored);
            } else {
                let stored = match value {
                    SetValue::Default => extract(record.bits, slot_defaults_load(slot_defaults_offset(slot, record.offset), record.bits.width)),
                    SetValue::Value(value) => value,
                };
                write(record.bits, record.save, bank, bank_record_offset(bank, slot, record.offset), ImageOffset(record_offset(slot, record.offset)), stored);
            }
            1
        }
        Kind::Unassigned | Kind::Delegated => 0,
    }
}

/// Getter arm body: `id` is the stock r12, `bank` r1, `defaults` r2 and `slot` r3 at the arm.
///
/// # Safety
/// The bank image for `bank` must be mapped at 0x96cc64 + 0x1c2 * bank, the slot-defaults record
/// for `slot` at 0x7cb804 + 0x28 * slot, and the defaults table at 0x81b074.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_get(id: u32, bank: u32, defaults: u32, slot: u32) -> u32 {
    get(PropertyId(id), BankIndex(bank), if defaults == 1 { ReadMode::Default } else { ReadMode::Live }, SlotNumber(slot))
}

/// Setter arm body: `id`, `value`, `bank` and `slot` are the stock r4, r5, r6 and r8 at the arm.
///
/// # Safety
/// The bank image for `bank` must be mapped at 0x96cc64 + 0x1c2 * bank, for bank 0 the
/// previous-value image at 0x89ec00, the slot-defaults record for `slot` at 0x7cb804 + 0x28 * slot,
/// and the defaults table at 0x81b074.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_set(id: u32, value: u32, bank: u32, slot: u32) -> u32 {
    set(PropertyId(id), if value == DEFAULT_REQUEST { SetValue::Default } else { SetValue::Value(value) }, BankIndex(bank), SlotNumber(slot))
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
