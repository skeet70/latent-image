//! The stock boundary: where the registry, the records and the shared block live, the handle
//! word, and the stock entries other tasks share.

pub mod layout;
pub mod record;
pub mod shared;

use xm1_saclac_interface::stock::{record_handle_from_a2, record_handle_word};
use xm1_saclac_interface::RecordHandle;

use crate::entries::{Grants, OpenMode, SharedHeaderByte, StockEntries};
use crate::field::Value;
use crate::handle::{Handle, Scope, Serial, Slot, Tag};
use crate::image::Header;
use crate::memory::{Block, Location, Memory};
use crate::values::MeasurementSelection;

const REGISTRY: usize = 0x915d38;
const RECORDS: usize = 0x96ebd0;
const SHARED: usize = 0x91904c;

const _: () = assert!(REGISTRY % 4 == 0 && RECORDS % 4 == 0 && layout::RECORD_BYTES % 4 == 0 && SHARED % 4 == 0);

/// The 43 grant bytes saclac opens its slots with (0xf14a50).
const SACLAC_GRANTS: usize = 0x81aa6c;

const INVALID: u32 = u32::MAX;

/// Bits 0..4 slot (0x75a984), 4..16 serial (0x75a99c), 16 owner (0x75a9e4), 17 writable
/// (0x75a9cc), 18..24 scope (0x75a9b4), 24..32 tag (0x75a868).
pub fn handle(handle: RecordHandle) -> Option<Handle> {
    let word = record_handle_word(handle);
    if word == INVALID {
        return None;
    }
    Some(Handle {
        slot: Slot::new((word & 0xf) as u8)?,
        serial: Serial::new(((word >> 4) & 0xfff) as u16)?,
        owner: word & 1 << 16 != 0,
        writable: word & 1 << 17 != 0,
        scope: Scope::from_word(((word >> 18) & 0x3f) as u8),
        tag: Tag((word >> 24) as u8),
    })
}

pub fn record_handle(handle: Handle) -> RecordHandle {
    let word = handle.slot.index() as u32
        | u32::from(handle.serial.get()) << 4
        | u32::from(handle.owner) << 16
        | u32::from(handle.writable) << 17
        | u32::from(handle.scope.word()) << 18
        | u32::from(handle.tag.0) << 24;
    record_handle_from_a2(word)
}

const fn address<T: Value>(at: Location<T>) -> usize {
    let base = match at.block() {
        Block::Registry => REGISTRY,
        Block::Record(record) => RECORDS + layout::RECORD_BYTES * record.index(),
        Block::Shared => SHARED,
    };
    base + at.offset() as usize
}

/// The stock objects, which other tasks access concurrently; every access is volatile.
pub struct StockMemory;

impl Memory for StockMemory {
    fn read_u8(&self, at: Location<u8>) -> u8 { unsafe { core::ptr::read_volatile(address(at) as *const u8) } }

    fn read_u16(&self, at: Location<u16>) -> u16 { unsafe { core::ptr::read_volatile(address(at) as *const u16) } }

    fn read_u32(&self, at: Location<u32>) -> u32 { unsafe { core::ptr::read_volatile(address(at) as *const u32) } }

    fn write_u8(&mut self, at: Location<u8>, value: u8) { unsafe { core::ptr::write_volatile(address(at) as *mut u8, value) } }

    fn write_u16(&mut self, at: Location<u16>, value: u16) { unsafe { core::ptr::write_volatile(address(at) as *mut u16, value) } }

    fn write_u32(&mut self, at: Location<u32>, value: u32) { unsafe { core::ptr::write_volatile(address(at) as *mut u32, value) } }
}

/// The stock entries, entered at their X-M1 1.50 addresses.
pub struct Stock;

const fn header_byte_entry(byte: SharedHeaderByte) -> usize {
    match byte {
        SharedHeaderByte::At31 => 0x74e648,
        SharedHeaderByte::At46 => 0x74f9a4,
        SharedHeaderByte::At47 => 0x74fa2c,
        SharedHeaderByte::At61 => 0x74fc64,
        SharedHeaderByte::At63 => 0x7503ec,
    }
}

const MEASUREMENT_SELECTION: usize = 0x74e5c0;

const LENS_DESIGNATION: usize = 0x758c68;
const REPLACE_HEADER: usize = 0x74e074;
const OPEN: usize = 0x75a63c;
const CLOSE: usize = 0x75a6cc;
const INITIALIZE: usize = 0x756720;

const fn open_mode(mode: OpenMode) -> u32 {
    match mode {
        OpenMode::First => 0,
        OpenMode::Second => 1,
        OpenMode::Copy => 3,
        OpenMode::Selected => 4,
    }
}

const fn grant_table(grants: Grants) -> usize {
    match grants {
        Grants::Saclac => SACLAC_GRANTS,
    }
}

impl StockEntries for Stock {
    fn measurement_selection(&mut self, handle: RecordHandle) -> MeasurementSelection {
        let entry: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(MEASUREMENT_SELECTION as *const ()) };
        MeasurementSelection::from_byte(entry(record_handle_word(handle)) as u8)
    }

    fn header_byte(&mut self, handle: RecordHandle, byte: SharedHeaderByte) -> u8 {
        let entry: extern "C" fn(u32) -> u32 = unsafe { core::mem::transmute(header_byte_entry(byte) as *const ()) };
        entry(record_handle_word(handle)) as u8
    }

    fn lens_designation(&mut self, handle: RecordHandle) -> [u8; 28] {
        /// 0x758c68 stores it with `stm`.
        #[repr(C, align(4))]
        struct Designation([u8; 28]);
        let entry: extern "C" fn(u32, *mut Designation) = unsafe { core::mem::transmute(LENS_DESIGNATION as *const ()) };
        let mut designation = Designation([0; 28]);
        entry(record_handle_word(handle), &mut designation);
        designation.0
    }

    fn replace_header(&mut self, handle: RecordHandle, staged: Header<'_>) {
        let entry: extern "C" fn(u32, *const u8) = unsafe { core::mem::transmute(REPLACE_HEADER as *const ()) };
        entry(record_handle_word(handle), staged.bytes().as_ptr());
    }

    fn open(&mut self, mode: OpenMode, grants: Grants) -> RecordHandle {
        let entry: extern "C" fn(u32, *const u8) -> u32 = unsafe { core::mem::transmute(OPEN as *const ()) };
        record_handle_from_a2(entry(open_mode(mode), grant_table(grants) as *const u8))
    }

    fn close(&mut self, handle: RecordHandle) {
        let entry: extern "C" fn(u32) = unsafe { core::mem::transmute(CLOSE as *const ()) };
        entry(record_handle_word(handle));
    }

    fn initialize(&mut self) {
        let entry: extern "C" fn() = unsafe { core::mem::transmute(INITIALIZE as *const ()) };
        entry();
    }
}

/// The view over the stock objects.
pub const fn profile_records() -> crate::view::ProfileRecords<StockMemory> { crate::view::ProfileRecords::new(StockMemory) }
