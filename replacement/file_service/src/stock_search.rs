//! The stock boundary of the directory search workers: the request header the media task
//! passes, the media-state context and the retained search, memory and time services.

use core::mem::MaybeUninit;

use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::search::{self, Entry, EntryOffset, Field, Port, Record, ENTRY_BYTES, TESTED, TESTED_BYTES};
use crate::stock::{address, file_service_copy_memory, file_service_fill_memory, read_half, read_word, write_byte, write_word};
use crate::workers::Offset;

const CONTEXT: u32 = 0x910204;
const CONTEXT_ROUNDING: u32 = 0x16bf8;

extern "C" {
    fn file_service_little_endian(value: u32) -> u32;
    fn file_service_divide(dividend: u32, divisor: u32) -> u32;
    fn file_service_format_timestamp(destination: u32, stamp: *const u8, byte: u32);
    fn file_service_find_first(path: u32, pattern: u32, entry: *mut u8) -> u32;
    fn file_service_find_next(entry: *mut u8) -> u32;
    fn file_service_find_first_entry(path: u32, pattern: u32, entry: *mut u8, flag: u32) -> u32;
    fn file_service_find_next_entry(entry: *mut u8) -> u32;
    fn file_service_name_rejected(name: *const u8, length: u32) -> u32;
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address.
struct StockSearch {
    header: u32,
    payload: u32,
}

impl StockSearch {
    fn new(header: u32) -> StockSearch {
        StockSearch { header, payload: read_word(header + 8) }
    }
}

impl Port for StockSearch {
    fn word(&self, at: Offset) -> u32 {
        read_word(self.payload + at.0)
    }

    fn address(&self, at: Offset) -> u32 {
        self.payload + at.0
    }

    fn context(&self) -> bool {
        read_word(CONTEXT) != 0
    }

    fn rounding(&self) -> u32 {
        read_word(read_word(CONTEXT) + CONTEXT_ROUNDING)
    }

    fn cleared_entry(&mut self) -> Entry {
        let mut entry = MaybeUninit::<[u8; ENTRY_BYTES]>::uninit();
        unsafe {
            file_service_fill_memory(address(entry.as_mut_ptr()), 0, ENTRY_BYTES as u32);
            Entry(entry.assume_init())
        }
    }

    fn clear(&mut self, output: Record, length: u32) {
        unsafe { file_service_fill_memory(output.0, 0, length) }
    }

    fn copy(&mut self, output: Record, entry: &Entry, length: u32) {
        unsafe { file_service_copy_memory(output.0, address(entry.0.as_ptr()), length) };
    }

    fn set_byte(&mut self, output: Record, at: Field, value: u8) {
        write_byte(output.0 + at.0, value)
    }

    fn set_word(&mut self, output: Record, at: Field, value: u32) {
        write_word(output.0 + at.0, value)
    }

    fn find_first(&mut self, path: u32, pattern: u32, entry: &mut Entry) -> RawStatus {
        RawStatus(unsafe { file_service_find_first(path, pattern, entry.0.as_mut_ptr()) })
    }

    fn find_next(&mut self, entry: &mut Entry) -> RawStatus {
        RawStatus(unsafe { file_service_find_next(entry.0.as_mut_ptr()) })
    }

    fn find_first_entry(&mut self, path: u32, pattern: u32, entry: &mut Entry, flag: u32) -> RawStatus {
        RawStatus(unsafe { file_service_find_first_entry(path, pattern, entry.0.as_mut_ptr(), flag) })
    }

    fn find_next_entry(&mut self, entry: &mut Entry) -> RawStatus {
        RawStatus(unsafe { file_service_find_next_entry(entry.0.as_mut_ptr()) })
    }

    fn rejected(&mut self, entry: &Entry) -> bool {
        unsafe { file_service_name_rejected(entry.0[TESTED.0..].as_ptr(), TESTED_BYTES) != 0 }
    }

    fn little_endian(&mut self, value: u32) -> u32 {
        unsafe { file_service_little_endian(value) }
    }

    fn divide(&mut self, dividend: u32, divisor: u32) -> u32 {
        unsafe { file_service_divide(dividend, divisor) }
    }

    fn format_time(&mut self, output: Record, at: Field, entry: &Entry, stamp: EntryOffset, byte: u8) {
        unsafe { file_service_format_timestamp(output.0 + at.0, entry.0[stamp.0..].as_ptr(), u32::from(byte)) }
    }

    fn translate(&mut self, raw: RawStatus) -> u32 {
        crate::stock_requests::xm1_file_service_translate(u32::from(read_half(self.header)), raw.0)
    }

    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            let function: extern "C" fn(u32) = unsafe { core::mem::transmute(callback.0 as usize) };
            function(status)
        }
    }
}

/// Entered at 0x712700 by the media task for code 0x2c08.
#[no_mangle]
pub extern "C" fn xm1_file_service_find_first_worker(header: u32) -> u32 {
    search::find_first(&mut StockSearch::new(header))
}

/// Entered at 0x7127e0 by the media task for code 0x2c09.
#[no_mangle]
pub extern "C" fn xm1_file_service_find_next_worker(header: u32) -> u32 {
    search::find_next(&mut StockSearch::new(header))
}

/// Entered at 0x71e3a4 by the media task for code 0x2c06.
#[no_mangle]
pub extern "C" fn xm1_file_service_find_first_entry_worker(header: u32) -> u32 {
    search::find_first_entry(&mut StockSearch::new(header))
}

/// Entered at 0x71e550 by the media task for code 0x2c07.
#[no_mangle]
pub extern "C" fn xm1_file_service_find_next_entry_worker(header: u32) -> u32 {
    search::find_next_entry(&mut StockSearch::new(header))
}
