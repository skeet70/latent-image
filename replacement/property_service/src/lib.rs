#![no_std]
//! Owned arms of the X-M1 1.50 property getter 0x61e110 and setter 0x624694. The stubs in
//! `entry.S` run inside the stock frame and pop it themselves.
//!
//! The oracle's defect builds set one `property_defect_*` cfg each.

mod property_table;
mod table;

use property_table::{PROPERTIES, SLOTS, SPECIALS};
use table::{
    BankDefault, BankOffset, Bits, ConfigOffset, FieldDefault, Hook, HookedField, Kind, ProfileSelector, RecordOffset, Save, SlotDefault, SlotIndex,
    SlotRecord, Source, SpecialField, SpecialIndex, StockDefault, Width, WriteOnlyPair,
};

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

const RECORD_ZERO: SlotNumber = SlotNumber(0);

#[derive(Clone, Copy)]
enum ReadMode {
    Live,
    Default,
}

#[derive(Clone, Copy)]
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
    fn xm1_tail_defaults_load16(offset: ImageOffset) -> u32;
    fn xm1_config_load8(offset: ConfigOffset) -> u32;
    fn xm1_stock_61df44() -> u32;
    fn xm1_stock_61d8a4() -> u32;
    fn xm1_stock_61d59c() -> u32;
    fn xm1_stock_61d618() -> u32;
    fn xm1_stock_61d5f0() -> u32;
    fn xm1_stock_61d65c(selector: ProfileSelector) -> u32;
}

#[inline(always)]
fn entry(id: PropertyId) -> Option<Kind> {
    match PROPERTIES.get(id.0.wrapping_sub(1) as usize) {
        Some(entry) => entry.kind(),
        None => None,
    }
}

#[inline(always)]
fn slot_record(index: SlotIndex) -> Option<SlotRecord> {
    match SLOTS.get(index.0 as usize) {
        Some(slot) => slot.record(),
        None => None,
    }
}

#[inline(always)]
fn special(index: SpecialIndex) -> Option<SpecialField> {
    match SPECIALS.get(index.0 as usize) {
        Some(special) => special.field(),
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
        BankDefault::Constant(value) => value,
        BankDefault::TailByte(offset) => xm1_tail_defaults_load8(ImageOffset(offset.0)),
    }
}

unsafe fn field_default(default: FieldDefault) -> u32 {
    match default {
        FieldDefault::Constant(value) => value,
        FieldDefault::TailByte(offset) => xm1_tail_defaults_load8(ImageOffset(offset.0)),
        FieldDefault::TailHalf(offset) => xm1_tail_defaults_load16(ImageOffset(offset.0)),
    }
}

unsafe fn stock_default(stock: StockDefault) -> u32 {
    match stock {
        StockDefault::Stock61df44 => xm1_stock_61df44(),
        StockDefault::Stock61d8a4 => xm1_stock_61d8a4(),
        StockDefault::Stock61d59c => xm1_stock_61d59c(),
        StockDefault::Stock61d618 => xm1_stock_61d618(),
        StockDefault::Stock61d5f0 => xm1_stock_61d5f0(),
        StockDefault::Stock61d65c(selector) => xm1_stock_61d65c(selector),
    }
}

/// The getter's configuration default: the whole byte.
unsafe fn config_default(bits: Bits, offset: ConfigOffset) -> u32 {
    let byte = xm1_config_load8(offset);
    if cfg!(property_defect_mask_config_default) {
        extract(bits, byte)
    } else {
        byte
    }
}

/// `container` with the field's bits replaced by the low bits of `value`, as BFI does.
fn insert(bits: Bits, container: u32, value: u32) -> u32 {
    let mask = bits.mask() << bits.lsb();
    (container & !mask) | ((value << bits.lsb()) & mask)
}

fn extract(bits: Bits, container: u32) -> u32 {
    (container >> bits.lsb()) & bits.mask()
}

/// One field a set writes: `live` is its offset in `bank`'s image and `saved` its offset in the
/// previous-value image, which is also its offset in bank 0. Only inlined functions take it.
#[derive(Clone, Copy)]
struct Target {
    bits: Bits,
    save: Save,
    bank: BankIndex,
    live: ImageOffset,
    saved: ImageOffset,
}

/// The setter's save and store of one field.
unsafe fn write(bits: Bits, save: Save, bank: BankIndex, live: ImageOffset, saved: ImageOffset, stored: u32) {
    if bank == BankIndex(0) && save == Save::Bank0Previous {
        let previous = bank_load(live, bits.width());
        let copied = if bits.whole() { previous } else { insert(bits, save_load(saved, bits.width()), previous >> bits.lsb()) };
        save_store(saved, bits.width(), copied);
    }
    let container = if bits.whole() { stored } else { insert(bits, bank_load(live, bits.width()), stored) };
    bank_store(live, bits.width(), container);
}

#[inline(always)]
unsafe fn write_to(target: Target, stored: u32) {
    write(target.bits, target.save, target.bank, target.live, target.saved, stored);
}

/// A default request with a stock default: the provisional write, then the leaf's result written
/// with a second save, which copies the provisional bits.
#[inline(always)]
unsafe fn write_stock_default(target: Target, provisional: u32, stock: StockDefault) {
    if !cfg!(property_defect_drop_provisional) {
        write_to(target, provisional);
    }
    let result = stock_default(stock);
    let save = if cfg!(property_defect_drop_second_save) { Save::None } else { target.save };
    write_to(Target { save, ..target }, result);
}

/// A default request with a configuration default: the provisional write, then the byte's low bits
/// stored without a save.
#[inline(always)]
unsafe fn write_config_default(target: Target, provisional: u32, offset: ConfigOffset) {
    write_to(target, provisional);
    let save = if cfg!(property_defect_config_save) { target.save } else { Save::None };
    write_to(Target { save, ..target }, xm1_config_load8(offset));
}

unsafe fn get_special(field: SpecialField, bank: BankIndex, mode: ReadMode) -> u32 {
    let (bits, offset, default) = match field {
        SpecialField::Hooked(hooked) => (hooked.bits, hooked.offset, hooked.source),
        SpecialField::ReadOnly(read_only) => (read_only.bits, read_only.offset, Source::Constant(read_only.default)),
        SpecialField::WriteOnlyPair(_) => return 0,
    };
    match mode {
        ReadMode::Live => extract(bits, bank_load(bank_offset(bank, offset), bits.width())),
        ReadMode::Default => match default {
            Source::Constant(value) => value,
            Source::Stock(stock, _) => stock_default(stock),
            Source::Config(config, _) => config_default(bits, config),
        },
    }
}

#[inline(always)]
unsafe fn set_hooked(field: HookedField, value: SetValue, bank: BankIndex) {
    let target = Target { bits: field.bits, save: field.save, bank, live: bank_offset(bank, field.offset), saved: bank_offset(BankIndex(0), field.offset) };
    let stored = match (value, field.source) {
        (SetValue::Value(value), _) | (SetValue::Default, Source::Constant(value)) => value,
        (SetValue::Default, Source::Stock(stock, provisional)) => return write_stock_default(target, provisional, stock),
        (SetValue::Default, Source::Config(config, provisional)) => return write_config_default(target, provisional, config),
    };
    write_to(target, stored);
    if let Hook::GatedResave(gate) = field.hook {
        if (xm1_config_load8(gate) == 0) != cfg!(property_defect_invert_gate) {
            write_to(target, stored);
        }
    }
}

/// One byte of a write-only pair.
#[inline(always)]
unsafe fn write_pair_byte(pair: WriteOnlyPair, offset: BankOffset, value: SetValue, bank: BankIndex) {
    let stored = match value {
        SetValue::Default => pair.default,
        SetValue::Value(value) => value,
    };
    write(pair.bits, pair.save, bank, bank_offset(bank, offset), bank_offset(BankIndex(0), offset), stored);
}

/// Whether the setter stub calls 0x61da3c before and 0x61d944 after a set of `field` on `bank`.
#[inline(always)]
fn bracketed(field: SpecialField, bank: BankIndex) -> bool {
    bank == BankIndex(0)
        && match field {
            SpecialField::Hooked(hooked) => matches!(hooked.hook, Hook::Bank0Bracket),
            SpecialField::WriteOnlyPair(_) => true,
            SpecialField::ReadOnly(_) => false,
        }
}

#[inline(always)]
fn special_of(id: PropertyId) -> Option<SpecialField> {
    match entry(id) {
        Some(Kind::Special(index)) => special(index),
        _ => None,
    }
}

fn set_value(value: u32) -> SetValue {
    if value == DEFAULT_REQUEST {
        SetValue::Default
    } else {
        SetValue::Value(value)
    }
}

unsafe fn get(id: PropertyId, bank: BankIndex, mode: ReadMode, slot: SlotNumber) -> u32 {
    match entry(id) {
        Some(Kind::Field(field)) => match mode {
            ReadMode::Default => field_default(field.default),
            ReadMode::Live => extract(field.bits, bank_load(bank_offset(bank, field.offset), field.bits.width())),
        },
        Some(Kind::SlotField(slot_field)) => {
            let Some(record) = slot_record(slot_field.index) else { return 0 };
            let slot = if slot_field.mirror { RECORD_ZERO } else { slot };
            match (slot == BANK_BRANCH, mode, record.default) {
                (_, ReadMode::Default, SlotDefault::Stock(stock)) => stock_default(stock),
                (true, ReadMode::Default, SlotDefault::Record) => bank_default(record.bank_default),
                (true, ReadMode::Live, _) => extract(slot_field.bits, bank_load(bank_offset(bank, slot_field.offset), slot_field.bits.width())),
                (false, ReadMode::Default, SlotDefault::Record) => extract(record.bits, slot_defaults_load(slot_defaults_offset(slot, record.offset), record.bits.width())),
                (false, ReadMode::Live, _) => extract(record.bits, bank_load(bank_record_offset(bank, slot, record.offset), record.bits.width())),
            }
        }
        Some(Kind::Special(index)) => match special(index) {
            Some(field) => get_special(field, bank, mode),
            None => 0,
        },
        Some(Kind::Unassigned) | None => 0,
    }
}

unsafe fn set(id: PropertyId, value: SetValue, bank: BankIndex, slot: SlotNumber) -> u32 {
    match entry(id) {
        Some(Kind::Field(field)) => {
            let stored = match value {
                SetValue::Default => field_default(field.default),
                SetValue::Value(value) => value,
            };
            write(field.bits, field.save, bank, bank_offset(bank, field.offset), bank_offset(BankIndex(0), field.offset), stored);
            1
        }
        Some(Kind::SlotField(slot_field)) => {
            let Some(record) = slot_record(slot_field.index) else { return 0 };
            let record_target = |slot: SlotNumber| Target {
                bits: record.bits,
                save: record.save,
                bank,
                live: bank_record_offset(bank, slot, record.offset),
                saved: ImageOffset(record_offset(slot, record.offset)),
            };
            let record_default = |slot: SlotNumber| extract(record.bits, slot_defaults_load(slot_defaults_offset(slot, record.offset), record.bits.width()));
            let target = if slot == BANK_BRANCH {
                Target { bits: slot_field.bits, save: record.save, bank, live: bank_offset(bank, slot_field.offset), saved: bank_offset(BankIndex(0), slot_field.offset) }
            } else {
                record_target(slot)
            };
            match value {
                SetValue::Value(value) => write_to(target, value),
                SetValue::Default => {
                    let stage2 = if slot == BANK_BRANCH { bank_default(record.bank_default) } else { record_default(slot) };
                    match record.default {
                        SlotDefault::Record => write_to(target, stage2),
                        SlotDefault::Stock(stock) => write_stock_default(target, stage2, stock),
                    }
                }
            }
            if slot_field.mirror {
                let stored = match value {
                    SetValue::Value(value) => value,
                    SetValue::Default => record_default(RECORD_ZERO),
                };
                write_to(record_target(RECORD_ZERO), stored);
            }
            1
        }
        Some(Kind::Special(index)) => match special(index) {
            Some(SpecialField::Hooked(field)) => {
                set_hooked(field, value, bank);
                1
            }
            Some(field @ SpecialField::WriteOnlyPair(pair)) => {
                write_pair_byte(pair, pair.first, value, bank);
                if !bracketed(field, bank) {
                    write_pair_byte(pair, pair.second, value, bank);
                }
                1
            }
            Some(SpecialField::ReadOnly(_)) | None => 0,
        },
        Some(Kind::Unassigned) | None => 0,
    }
}

/// Getter arm body: `id` is the stock r12, `bank` r1, `defaults` r2 and `slot` r3 at the arm.
///
/// # Safety
/// The bank image for `bank` must be mapped at 0x96cc64 + 0x1c2 * bank, the slot-defaults records
/// for `slot` and slot 0 at 0x7cb804 + 0x28 * slot, the defaults table at 0x81b074, and the
/// configuration bytes [0x96b400, 0x96b600). The stock leaves 0x61df44, 0x61d8a4, 0x61d59c,
/// 0x61d618, 0x61d5f0 and 0x61d65c must be the image4 functions, which follow AAPCS, read one
/// configuration byte and use 4 bytes of stack.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_get(id: u32, bank: u32, defaults: u32, slot: u32) -> u32 {
    get(PropertyId(id), BankIndex(bank), if defaults == 1 { ReadMode::Default } else { ReadMode::Live }, SlotNumber(slot))
}

/// Setter arm body up to the setter stub's close call: `id`, `value`, `bank` and `slot` are the
/// stock r4, r5, r6 and r8 at the arm. When `xm1_set_bracketed` returned 1 it runs after the open
/// call, and `slot` is not the stock r8.
///
/// # Safety
/// As `xm1_property_get`, and for bank 0 the previous-value image must be mapped at 0x89ec00.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_set(id: u32, value: u32, bank: u32, slot: u32) -> u32 {
    set(PropertyId(id), set_value(value), BankIndex(bank), SlotNumber(slot))
}

/// 1 when the setter stub brackets the set with 0x61da3c and 0x61d944 (a bank-0 set of a
/// bracketed field or a write-only pair), else 0.
#[no_mangle]
pub extern "C" fn xm1_set_bracketed(id: u32, bank: u32) -> u32 {
    match special_of(PropertyId(id)) {
        Some(field) => bracketed(field, BankIndex(bank)) as u32,
        None => 0,
    }
}

/// Setter arm body after the setter stub's close call: a write-only pair's second byte. Returns
/// the stock r0, 1.
///
/// # Safety
/// As `xm1_property_set`.
#[no_mangle]
pub unsafe extern "C" fn xm1_property_set_after(id: u32, value: u32, bank: u32) -> u32 {
    if let Some(SpecialField::WriteOnlyPair(pair)) = special_of(PropertyId(id)) {
        write_pair_byte(pair, pair.second, set_value(value), BankIndex(bank));
    }
    1
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
