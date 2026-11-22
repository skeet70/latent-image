//! The stock boundary of the catalog-count workers: the media-state context tables they read
//! and the retained path, extension, cluster and position services.

use crate::completion::RawStatus;
use crate::counts::{self, Directory, ExtensionKey, FileNumber, Frame, Lookup, Offset, Page, Port, Window};
use crate::requests::Callback;
use crate::stock::{
    address, file_service_catalog_locate, file_service_extension_key, file_service_fill_memory, read_byte, read_half,
    read_word, write_word,
};
use crate::stock_workers::file_service_split_path;

const CONTEXT: u32 = 0x910204;
const CONTEXT_DIRECTORIES: u32 = 0x2604;
const CONTEXT_MARKED_WINDOW: u32 = 0x2178;
const CONTEXT_UNMARKED_WINDOW: u32 = 0x218c;
const CONTEXT_DIRECTORY_FILES: u32 = 0x35a4;
const CONTEXT_PAGE_FILES: u32 = 0x127d6;
const CONTEXT_GROUPS: u32 = 0x12fa6;
const CONTEXT_GROUP_STARTS: u32 = 0x1577e;
const CONTEXT_PAGE_FLAGS: u32 = 0x15846;
const FIRST_NUMBERED: u32 = 100;
const EXTENSION: usize = 8;

extern "C" {
    fn file_service_extension_byte4(key: u32) -> u32;
    fn file_service_extension_byte5(key: u32) -> u32;
    fn file_service_directory_number(directory: *const u8, word: u32) -> u32;
    fn file_service_page_cluster(page: u32, cluster: *mut u32) -> u32;
    fn file_service_cluster_path(cluster: u32, path: *mut u8, marker: u32, bare: u32) -> u32;
    fn file_service_text_difference(first: *const u8, second: *const u8) -> u32;
    fn file_service_file_number(pattern: *const u8) -> u32;
}

/// The record 0x7189b4 reads and updates.
#[repr(C)]
struct StockLookup {
    kind: u32,
    cluster: u32,
    listed: u32,
    window_first: u32,
    window_last: u32,
    wanted: u32,
    page: u32,
    pattern: u32,
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address.
struct StockCounts {
    header: u32,
    payload: u32,
}

impl StockCounts {
    fn new(header: u32) -> StockCounts {
        StockCounts { header, payload: read_word(header + 8) }
    }

    fn context_word(&self) -> u32 {
        read_word(CONTEXT)
    }
}

impl Port for StockCounts {
    fn word(&self, at: Offset) -> u32 {
        read_word(self.payload + at.0)
    }

    fn address(&self, at: Offset) -> u32 {
        self.payload + at.0
    }

    fn store(&mut self, address: u32, value: u32) {
        write_word(address, value)
    }

    fn context(&self) -> bool {
        self.context_word() != 0
    }

    fn directory_entry(&self, directory: Directory) -> u32 {
        read_word(self.context_word() + CONTEXT_DIRECTORIES + 4 * u32::from(directory.0))
    }

    fn window(&self, window: Window) -> (Page, Page) {
        let at = self.context_word()
            + match window {
                Window::Marked => CONTEXT_MARKED_WINDOW,
                Window::Unmarked => CONTEXT_UNMARKED_WINDOW,
            };
        (Page(read_word(at)), Page(read_word(at + 4)))
    }

    fn group(&self, number: FileNumber) -> u8 {
        read_byte(self.context_word().wrapping_add(number.0).wrapping_add(CONTEXT_GROUPS))
    }

    fn group_start(&self, group: u8) -> FileNumber {
        FileNumber(u32::from(read_half(self.context_word() + CONTEXT_GROUP_STARTS + 2 * u32::from(group))))
    }

    fn page_flags(&self, index: u32) -> u8 {
        read_byte(self.context_word().wrapping_add(CONTEXT_PAGE_FLAGS).wrapping_add(index))
    }

    fn page_files(&self, page: Page) -> u16 {
        read_half(self.context_word() + CONTEXT_PAGE_FILES + 2 * (page.0 - FIRST_NUMBERED))
    }

    fn directory_files(&self, directory: Directory) -> u16 {
        read_half(self.context_word() + CONTEXT_DIRECTORY_FILES + 2 * (u32::from(directory.0) - FIRST_NUMBERED))
    }

    fn clear(&mut self, buffer: &mut [u8; 32]) {
        unsafe { file_service_fill_memory(address(buffer.as_mut_ptr()), 0, 32) }
    }

    fn split_path(&mut self, path: u32, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe {
            file_service_split_path(path, frame.directory.as_mut_ptr(), frame.pattern.as_mut_ptr(), &mut frame.marker, frame.prefix.as_mut_ptr(), &mut frame.secondary)
        })
    }

    fn extension_key(&mut self, frame: &Frame) -> ExtensionKey {
        ExtensionKey(unsafe { file_service_extension_key(address(frame.pattern[EXTENSION..].as_ptr())) })
    }

    fn extension_byte4(&mut self, key: ExtensionKey) -> u32 {
        unsafe { file_service_extension_byte4(key.0) }
    }

    fn extension_byte5(&mut self, key: ExtensionKey) -> u32 {
        unsafe { file_service_extension_byte5(key.0) }
    }

    fn directory_number(&mut self, frame: &Frame) -> u32 {
        unsafe { file_service_directory_number(frame.directory.as_ptr(), frame.marker) }
    }

    fn reserved_cluster(&mut self, page: Page, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe { file_service_page_cluster(page.0, &mut frame.cluster.0) })
    }

    fn cluster_path(&mut self, frame: &mut Frame) -> u32 {
        unsafe { file_service_cluster_path(frame.cluster.0, frame.query.as_mut_ptr(), frame.marker, 0) }
    }

    fn text_difference(&mut self, frame: &Frame) -> u32 {
        unsafe { file_service_text_difference(frame.query.as_ptr(), frame.prefix.as_ptr()) }
    }

    fn directory_position(&mut self, lookup: Lookup, frame: &mut Frame) -> RawStatus {
        let mut stock = StockLookup {
            kind: 1,
            cluster: lookup.cluster.0,
            listed: u32::from(lookup.listed),
            window_first: 0,
            window_last: 4999,
            wanted: 0xffff_ffff,
            page: lookup.page.0,
            pattern: frame.pattern.as_ptr() as u32,
        };
        RawStatus(unsafe { file_service_catalog_locate(address(&mut stock), address(&mut frame.position)) })
    }

    fn file_number(&mut self, frame: &Frame) -> FileNumber {
        FileNumber(unsafe { file_service_file_number(frame.pattern.as_ptr()) })
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

/// Entered at 0x718de4 by the media task for code 0x2c0e, and by `bl` from 0x71a93c with a
/// request on its stack whose callback is 0; that caller reads the returned status.
#[no_mangle]
pub extern "C" fn xm1_file_service_file_position_worker(header: u32) -> u32 {
    counts::file_position(&mut StockCounts::new(header))
}

/// Entered at 0x718a8c by the media task for code 0x2c0f.
#[no_mangle]
pub extern "C" fn xm1_file_service_file_counts_worker(header: u32) -> u32 {
    counts::file_counts(&mut StockCounts::new(header))
}
