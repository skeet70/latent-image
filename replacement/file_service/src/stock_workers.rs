//! The stock boundary of the handle workers: the media-state context, the request header
//! the media task passes, and the retained path, filesystem and media-state services.

use crate::completion::RawStatus;
use crate::handles::FileIndex;
use crate::requests::Callback;
use crate::stock::{read_byte, read_word, write_word};
use crate::workers::{self, Name, OpenMode, Offset, PathFrame, Port};

const CONTEXT: u32 = 0x910204;
const CONTEXT_DEVICE: u32 = 0x114;
const CONTEXT_FLAGGED_CLOSE: u32 = 0x21cc;
const CONTEXT_DIRECTORY_RECORDS: u32 = 0x21da;
/// Nonzero words, one per directory number, at context + 0x2604 + 4 * number.
const CONTEXT_DIRECTORIES: u32 = 0x2604;
const FILE_RECORD_BYTES: u32 = 16;
const FILE_KIND: u32 = 0xc;

extern "C" {
    fn file_service_cache_release(index: u32);
    fn file_service_parse_path(parsed: *mut u32, path: u32) -> u32;
    fn file_service_directory_slot(parsed: u32) -> u32;
    pub(crate) fn file_service_split_path(path: u32, directory: *mut u8, name: *mut u8, word: *mut u32, prefix: *mut u8, flag: *mut u32) -> u32;
    fn file_service_directory_number(directory: *const u8, word: u32) -> u32;
    fn file_service_make_directory(directory: u32, record: u32);
    fn file_service_fs_open(name: *const u8, mode: u32, index: u32, word: u32) -> u32;
    fn file_service_register_written(index: u32, name: *const u8);
    fn file_service_fs_write(index: u32, source: u32, length: u32, raw: *mut u32) -> u32;
    fn file_service_fs_seek(index: u32, pad: u32, low: u32, high: u32, whence: u32) -> u32;
    fn file_service_fs_close(index: u32) -> u32;
    fn file_service_fs_close_named(index: u32, word: u32, name: u32) -> u32;
    fn file_service_fs_close_entry(index: u32, attribute: u32, first: u32, second: u32) -> u32;
    fn file_service_fs_release(index: u32) -> u32;
    fn file_service_media_refresh(device: u32, mode: u32);
    fn file_service_after_written_close(index: u32);
    fn file_service_rescan() -> u32;
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address.
struct StockWorker {
    header: u32,
    payload: u32,
}

impl StockWorker {
    fn new(header: u32) -> StockWorker {
        StockWorker { header, payload: read_word(header + 8) }
    }

    fn context_word(&self) -> u32 {
        read_word(CONTEXT)
    }

    fn file_record(&self, index: FileIndex) -> u32 {
        self.context_word() + FILE_RECORD_BYTES * index.0
    }
}

impl Port for StockWorker {
    fn word(&self, at: Offset) -> u32 {
        read_word(self.payload + at.0)
    }

    fn set_word(&mut self, at: Offset, value: u32) {
        write_word(self.payload + at.0, value)
    }

    fn byte(&self, at: Offset) -> u8 {
        read_byte(self.payload + at.0)
    }

    fn address(&self, at: Offset) -> u32 {
        self.payload + at.0
    }

    fn name(&self, at: Offset) -> Name {
        let mut name = [0; 48];
        for (index, byte) in name.iter_mut().enumerate() {
            *byte = read_byte(self.payload + at.0 + index as u32);
        }
        name
    }

    fn store(&mut self, address: u32, value: u32) {
        write_word(address, value)
    }

    fn context(&self) -> bool {
        self.context_word() != 0
    }

    fn file_mode(&self, index: FileIndex) -> u32 {
        read_word(self.file_record(index))
    }

    fn set_file_mode(&mut self, index: FileIndex, mode: u32) {
        write_word(self.file_record(index), mode)
    }

    fn file_kind(&self, index: FileIndex) -> u32 {
        read_word(self.file_record(index) + FILE_KIND)
    }

    fn mark_flagged_close(&mut self) {
        write_word(self.context_word() + CONTEXT_FLAGGED_CLOSE, 1)
    }

    fn directory_known(&self, directory: u16) -> bool {
        read_word(self.context_word() + CONTEXT_DIRECTORIES + 4 * u32::from(directory)) != 0
    }

    fn release_tag(&mut self, tag: u32) {
        crate::stock::xm1_file_service_tag_release(tag)
    }

    fn lookup(&mut self, tag: u32) -> FileIndex {
        FileIndex(crate::stock::xm1_file_service_map_lookup(tag))
    }

    fn unbind(&mut self, tag: u32) {
        crate::stock::xm1_file_service_map_unbind(tag)
    }

    fn bind(&mut self, tag: u32) -> FileIndex {
        FileIndex(crate::stock::xm1_file_service_map_bind(tag))
    }

    fn translate(&mut self, raw: RawStatus) -> u32 {
        crate::stock_requests::xm1_file_service_translate(u32::from(crate::stock::read_half(self.header)), raw.0)
    }

    fn complete(&mut self, callback: Callback, status: u32, value: u32) {
        if callback.0 != 0 {
            let function: extern "C" fn(u32, u32) = unsafe { core::mem::transmute(callback.0 as usize) };
            function(status, value)
        }
    }

    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            let function: extern "C" fn(u32) = unsafe { core::mem::transmute(callback.0 as usize) };
            function(status)
        }
    }

    fn done(&mut self) {
        crate::stock_requests::xm1_file_service_result_done();
    }

    fn parse_path(&mut self, frame: &mut PathFrame, path: u32) -> u32 {
        unsafe { file_service_parse_path(&mut frame.parsed, path) }
    }

    fn directory_slot(&mut self, parsed: u32) -> u32 {
        unsafe { file_service_directory_slot(parsed) }
    }

    fn split_path(&mut self, path: u32, frame: &mut PathFrame) {
        unsafe {
            file_service_split_path(path, frame.directory.as_mut_ptr(), frame.name.as_mut_ptr(), &mut frame.split_word, core::ptr::null_mut(), &mut frame.split_flag);
        }
    }

    fn directory_number(&mut self, frame: &PathFrame) -> u32 {
        unsafe { file_service_directory_number(frame.directory.as_ptr(), frame.split_word) }
    }

    fn make_directory(&mut self, directory: u16, slot: u32) {
        let record = self.context_word() + FILE_RECORD_BYTES * slot + CONTEXT_DIRECTORY_RECORDS;
        unsafe { file_service_make_directory(u32::from(directory), record) }
    }

    fn fs_open(&mut self, name: &Name, mode: OpenMode, index: FileIndex, word: u32) -> RawStatus {
        RawStatus(unsafe { file_service_fs_open(name.as_ptr(), u32::from(mode.0), index.0, word) })
    }

    fn register_written(&mut self, index: FileIndex, name: &Name) {
        unsafe { file_service_register_written(index.0, name.as_ptr()) }
    }

    fn fs_write(&mut self, index: FileIndex, source: u32, length: u32) -> (u32, RawStatus) {
        let mut raw = 0;
        let count = unsafe { file_service_fs_write(index.0, source, length, &mut raw) };
        (count, RawStatus(raw))
    }

    fn fs_seek(&mut self, index: FileIndex, pad: u32, low: u32, high: u32, whence: u32) -> RawStatus {
        RawStatus(unsafe { file_service_fs_seek(index.0, pad, low, high, whence) })
    }

    fn fs_close(&mut self, index: FileIndex) -> RawStatus {
        RawStatus(unsafe { file_service_fs_close(index.0) })
    }

    fn fs_close_named(&mut self, index: FileIndex, word: u32, name: u32) -> RawStatus {
        RawStatus(unsafe { file_service_fs_close_named(index.0, word, name) })
    }

    fn fs_close_entry(&mut self, index: FileIndex, attribute: u8, first: u32, second: u32) -> RawStatus {
        RawStatus(unsafe { file_service_fs_close_entry(index.0, u32::from(attribute), first, second) })
    }

    fn fs_release(&mut self, index: FileIndex) -> RawStatus {
        RawStatus(unsafe { file_service_fs_release(index.0) })
    }

    fn cache_release(&mut self, index: FileIndex) {
        unsafe { file_service_cache_release(index.0) }
    }

    fn refresh_media(&mut self) {
        unsafe { file_service_media_refresh(read_word(self.context_word() + CONTEXT_DEVICE), 0) }
    }

    fn after_written_close(&mut self, index: FileIndex) {
        unsafe { file_service_after_written_close(index.0) }
    }

    fn rescan(&mut self) -> u32 {
        unsafe { file_service_rescan() }
    }
}

/// Entered at 0x7115a4 by the media task for code 0x2c17.
#[no_mangle]
pub extern "C" fn xm1_file_service_open_worker(header: u32) -> u32 {
    workers::open(&mut StockWorker::new(header))
}

/// Entered at 0x7116e4 by the media task for code 0x2c19.
#[no_mangle]
pub extern "C" fn xm1_file_service_write_worker(header: u32) -> u32 {
    workers::write(&mut StockWorker::new(header))
}

/// Entered at 0x711768 by the media task for code 0x2c1a.
#[no_mangle]
pub extern "C" fn xm1_file_service_close_worker(header: u32) -> u32 {
    workers::close(&mut StockWorker::new(header))
}

/// Entered at 0x711840 by the media task for code 0x2c1b.
#[no_mangle]
pub extern "C" fn xm1_file_service_close_entry_worker(header: u32) -> u32 {
    workers::close_entry(&mut StockWorker::new(header))
}

/// Entered at 0x71eca4 by the media task for code 0x2c1c.
#[no_mangle]
pub extern "C" fn xm1_file_service_seek_worker(header: u32) -> u32 {
    workers::seek(&mut StockWorker::new(header))
}

/// Entered at 0x71f134 by the media task for code 0x2c1d.
#[no_mangle]
pub extern "C" fn xm1_file_service_release_worker(header: u32) -> u32 {
    workers::release(&mut StockWorker::new(header))
}
