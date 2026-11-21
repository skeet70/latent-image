//! The stock boundary of the select-image worker: the request header, the media-state
//! context and the retained path, catalog and numbering services.

use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::selection::{self, ContextOffset, ExtensionKind, Frame, Page, Port, Position};
use crate::stock::{read_byte, read_half, read_word, write_half, write_word};
use crate::stock_workers::file_service_split_path;

const CONTEXT: u32 = 0x910204;
const PATH: u32 = 4;
const EXTENSION: usize = 8;

extern "C" {
    fn file_service_fill_memory(buffer: *mut u8, value: u32, length: u32);
    fn file_service_extension_key(extension: *const u8) -> u32;
    fn file_service_extension_byte4(kind: u32) -> u32;
    fn file_service_directory_number(directory: *const u8, word: u32) -> u32;
    fn file_service_page_cluster(page: u32, cluster: *mut u32) -> u32;
    fn file_service_cluster_path(cluster: u32, path: *mut u8, marker: u32, bare: u32) -> u32;
    fn file_service_text_difference(first: *const u8, second: *const u8) -> u32;
    fn file_service_catalog_locate(query: *mut selection::Query, position: *mut u32) -> u32;
    fn file_service_marked_before(position: u32) -> u32;
    fn file_service_page_counted(page: u32) -> u32;
    fn file_service_media_snapshot();
    fn file_service_page_selected(page: u32, selected: *mut u32) -> u32;
    fn file_service_catalog_load(query: *mut selection::Query, first: u32, second: u32) -> u32;
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address.
struct StockSelection {
    header: u32,
    payload: u32,
}

impl StockSelection {
    fn new(header: u32) -> StockSelection {
        StockSelection { header, payload: read_word(header + 8) }
    }

    fn at(&self, offset: ContextOffset) -> u32 {
        read_word(CONTEXT).wrapping_add(offset.0)
    }
}

impl Port for StockSelection {
    fn callback(&self) -> Callback {
        Callback(read_word(self.payload))
    }

    fn path(&self) -> u32 {
        self.payload + PATH
    }

    fn context(&self) -> bool {
        read_word(CONTEXT) != 0
    }

    fn word(&self, at: ContextOffset) -> u32 {
        read_word(self.at(at))
    }

    fn set_word(&mut self, at: ContextOffset, value: u32) {
        write_word(self.at(at), value)
    }

    fn half(&self, at: ContextOffset) -> u16 {
        read_half(self.at(at))
    }

    fn set_half(&mut self, at: ContextOffset, value: u16) {
        write_half(self.at(at), value)
    }

    fn byte(&self, at: ContextOffset) -> u8 {
        read_byte(self.at(at))
    }

    fn clear(&mut self, buffer: &mut [u8; 32]) {
        unsafe { file_service_fill_memory(buffer.as_mut_ptr(), 0, 32) }
    }

    fn split_path(&mut self, path: u32, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe {
            file_service_split_path(
                path,
                frame.directory.as_mut_ptr(),
                frame.pattern.as_mut_ptr(),
                &mut frame.marker,
                frame.prefix.as_mut_ptr(),
                &mut frame.secondary,
            )
        })
    }

    fn extension_kind(&mut self, frame: &Frame) -> ExtensionKind {
        ExtensionKind(unsafe { file_service_extension_key(frame.pattern[EXTENSION..].as_ptr()) })
    }

    fn kind_supported(&mut self, kind: ExtensionKind) -> u32 {
        unsafe { file_service_extension_byte4(kind.0) }
    }

    fn directory_number(&mut self, frame: &Frame) -> u32 {
        unsafe { file_service_directory_number(frame.directory.as_ptr(), frame.marker) }
    }

    fn page_cluster(&mut self, page: Page, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe { file_service_page_cluster(page.0, &mut frame.cluster) })
    }

    fn directory_path(&mut self, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe { file_service_cluster_path(frame.cluster, frame.found.as_mut_ptr(), frame.marker, 0) })
    }

    fn paths_differ(&mut self, frame: &Frame) -> bool {
        unsafe { file_service_text_difference(frame.found.as_ptr(), frame.prefix.as_ptr()) != 0 }
    }

    fn find(&mut self, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe { file_service_catalog_locate(&mut frame.query, &mut frame.position) })
    }

    fn marked_before(&mut self, position: Position) -> u32 {
        unsafe { file_service_marked_before(position.0) }
    }

    fn page_counted(&mut self, page: Page) -> u32 {
        unsafe { file_service_page_counted(page.0) }
    }

    fn refresh_snapshot(&mut self) {
        unsafe { file_service_media_snapshot() }
    }

    fn page_selected(&mut self, page: Page, frame: &mut Frame) -> RawStatus {
        RawStatus(unsafe { file_service_page_selected(page.0, &mut frame.selected) })
    }

    fn load_window(&mut self, frame: &mut Frame) {
        unsafe { file_service_catalog_load(&mut frame.query, 0, 0) };
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

/// Entered at 0x719098 by the media task for code 0x2c1f.
#[no_mangle]
pub extern "C" fn xm1_file_service_select_worker(header: u32) -> u32 {
    selection::select(&mut StockSelection::new(header))
}
