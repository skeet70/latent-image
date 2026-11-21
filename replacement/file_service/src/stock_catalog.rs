//! The stock boundary of the catalog workers: the media-state context, the request header the
//! media task passes, and the retained catalog, directory and memory services.

use crate::catalog::{self, CatalogMark, Directory, FindSelectors, GroupMask, Port, RebuildMode, ResolveFrame, Window, WindowQuery};
use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::stock::{read_byte, read_half, read_word, write_byte, write_word};
use crate::workers::Offset;

const CONTEXT: u32 = 0x910204;
const CONTEXT_SELECTION: u32 = 0x1f5;
/// Words per directory number at +0x208 and +0x11a8.
const CONTEXT_FIRST_TOTALS: u32 = 0x208;
const CONTEXT_SECOND_TOTALS: u32 = 0x11a8;
const CONTEXT_CATALOG_MARK: u32 = 0x21cc;
const CONTEXT_GROUP_MASK: u32 = 0x2210;
const CONTEXT_GROUP_COUNT: u32 = 0x2600;
/// Halfwords from directory 100.
const CONTEXT_DIRECTORY_COUNTS: u32 = 0x35a4;

extern "C" {
    fn file_service_catalog_rebuild(full: u32, mode: u32) -> u32;
    fn file_service_fill_memory(address: u32, value: u32, length: u32);
    fn file_service_catalog_find(mode: u32, commit: u32, step: u32, word: u32, name: u32, record: u32) -> u32;
    fn file_service_rescan() -> u32;
    fn file_service_page_selected(directory: u32, marked: *mut u32) -> u32;
    fn file_service_page_cluster(directory: u32, cluster: *mut u32) -> u32;
    fn file_service_directory_reserved(directory: u32, reserved: *mut u32);
    fn file_service_cluster_path(cluster: u32, name: *mut u8, reserved: u32, bare: u32) -> u32;
    fn file_service_catalog_window(query: *mut WindowQuery, visit: unsafe extern "C" fn(), window: *mut Window) -> u32;
    fn file_service_catalog_window_visit();
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address.
struct StockCatalog {
    header: u32,
    payload: u32,
}

impl StockCatalog {
    fn new(header: u32) -> StockCatalog {
        StockCatalog { header, payload: read_word(header + 8) }
    }

    fn context_word(&self) -> u32 {
        read_word(CONTEXT)
    }
}

impl Port for StockCatalog {
    fn word(&self, at: Offset) -> u32 {
        read_word(self.payload + at.0)
    }

    fn byte(&self, at: Offset) -> u8 {
        read_byte(self.payload + at.0)
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

    fn group_count(&self) -> u8 {
        read_byte(self.context_word().wrapping_add(CONTEXT_GROUP_COUNT))
    }

    fn set_group_mask(&mut self, mask: GroupMask) {
        write_word(self.context_word().wrapping_add(CONTEXT_GROUP_MASK), mask.0)
    }

    fn catalog_mark(&self) -> CatalogMark {
        CatalogMark(read_word(self.context_word() + CONTEXT_CATALOG_MARK))
    }

    fn set_catalog_mark(&mut self, mark: CatalogMark) {
        write_word(self.context_word() + CONTEXT_CATALOG_MARK, mark.0)
    }

    fn clear_selection(&mut self) {
        write_byte(self.context_word().wrapping_add(CONTEXT_SELECTION), 0)
    }

    fn directory_count(&self, directory: Directory) -> u16 {
        read_half(self.context_word() + CONTEXT_DIRECTORY_COUNTS + 2 * u32::from(directory.0 - 100))
    }

    fn directory_totals(&self, directory: Directory) -> (u32, u32) {
        let offset = 4 * u32::from(directory.0);
        (read_word(self.context_word() + CONTEXT_FIRST_TOTALS + offset), read_word(self.context_word() + CONTEXT_SECOND_TOTALS + offset))
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

    fn done(&mut self) {
        crate::stock_requests::xm1_file_service_result_done();
    }

    fn rebuild_catalog(&mut self, full: bool, mode: RebuildMode) -> RawStatus {
        RawStatus(unsafe { file_service_catalog_rebuild(u32::from(full), mode.0) })
    }

    fn fill(&mut self, address: u32, value: u8, length: u32) {
        unsafe { file_service_fill_memory(address, u32::from(value), length) }
    }

    fn find_entry(&mut self, selectors: FindSelectors, name: u32, record: u32) -> RawStatus {
        RawStatus(unsafe { file_service_catalog_find(selectors.mode, selectors.commit, selectors.step, selectors.word, name, record) })
    }

    fn complete_request(&mut self) -> RawStatus {
        RawStatus(unsafe { file_service_rescan() })
    }

    fn directory_marked(&mut self, directory: Directory, frame: &mut ResolveFrame) -> RawStatus {
        RawStatus(unsafe { file_service_page_selected(u32::from(directory.0), &mut frame.query.marked) })
    }

    fn reserved_cluster(&mut self, directory: Directory, frame: &mut ResolveFrame) -> RawStatus {
        RawStatus(unsafe { file_service_page_cluster(u32::from(directory.0), &mut frame.query.cluster) })
    }

    fn directory_reserved(&mut self, directory: Directory, frame: &mut ResolveFrame) {
        unsafe { file_service_directory_reserved(u32::from(directory.0), &mut frame.reserved) }
    }

    fn cluster_path(&mut self, frame: &mut ResolveFrame) -> RawStatus {
        RawStatus(unsafe { file_service_cluster_path(frame.query.cluster, frame.name.as_mut_ptr(), frame.reserved, 0) })
    }

    fn scan_window(&mut self, frame: &mut ResolveFrame) -> RawStatus {
        RawStatus(unsafe { file_service_catalog_window(&mut frame.query, file_service_catalog_window_visit, &mut frame.window) })
    }
}

/// Entered at 0x71a834 by the media task for code 0x2c2b.
#[no_mangle]
pub extern "C" fn xm1_file_service_select_groups_worker(header: u32) -> u32 {
    catalog::select_groups(&mut StockCatalog::new(header))
}

/// Entered at 0x71a8a0 by the media task for code 0x2c02.
#[no_mangle]
pub extern "C" fn xm1_file_service_rebuild_catalog_worker(header: u32) -> u32 {
    catalog::rebuild_catalog(&mut StockCatalog::new(header))
}

/// Entered at 0x71b8c0 by the media task for code 0x2c0d.
#[no_mangle]
pub extern "C" fn xm1_file_service_find_entry_worker(header: u32) -> u32 {
    catalog::find_entry(&mut StockCatalog::new(header))
}

/// Entered at 0x71b9b0 by the media task for code 0x2c05.
#[no_mangle]
pub extern "C" fn xm1_file_service_complete_request_worker(header: u32) -> u32 {
    catalog::complete_request(&mut StockCatalog::new(header))
}

/// Entered at 0x719614 by the media task for code 0x2c10.
#[no_mangle]
pub extern "C" fn xm1_file_service_resolve_index_worker(header: u32) -> u32 {
    catalog::resolve_index(&mut StockCatalog::new(header))
}
