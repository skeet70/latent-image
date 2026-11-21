//! The stock boundary of the path workers: the media-state context, the request header and
//! worker table row the media task passes, and the retained request, path, catalog and
//! filesystem services.

use crate::completion::RawStatus;
use crate::paths::{
    self, Cancel, DeviceState, DirectoryNumber, DirectoryRecord, Entry, EntryService, ExtensionId, FileNumber, Port, Rebuild, Split,
};
use crate::requests::Callback;
use crate::stock::{read_byte, read_half, read_word, write_byte, write_word};
use crate::stock_workers::file_service_split_path;
use crate::workers::Offset;

const CONTEXT: u32 = 0x910204;
const CONTEXT_DEVICE: u32 = 0x114;
const CONTEXT_FILENAME_CANCEL: u32 = 0x1f6;
const CONTEXT_METADATA_CANCEL: u32 = 0x1f7;
/// Words, one per directory number, at context + 0x2604 + 4 * number.
const CONTEXT_DIRECTORIES: u32 = 0x2604;

extern "C" {
    fn file_service_delete_file_request(header: u32) -> u32;
    fn file_service_delete_directory_request(header: u32, row: u32) -> u32;
    fn file_service_readonly_request(header: u32) -> u32;
    fn file_service_catalog_rebuild(mode: u32, zero: u32) -> u32;
    fn file_service_rescan() -> u32;
    fn file_service_path_wildcard(path: u32) -> u32;
    fn file_service_directory_number(directory: *const u8, word: u32) -> u32;
    fn file_service_media_refresh(device: u32, mode: u32);
    fn file_service_entry_update(record: u32, name: *const u8, mode: u32, value: u32, wildcard: u32, one: u32) -> u32;
    fn file_service_entry_update_one(record: u32, name: *const u8, mode: u32, value: u32, wildcard: u32, one: u32) -> u32;
    fn file_service_device_state(device: u32, state: *mut u8);
    fn file_service_file_number(name: *const u8) -> u32;
    fn file_service_extension_key(extension: u32) -> u32;
    fn file_service_extension_byte5(id: u32) -> u32;
    fn file_service_catalog_find(first: u32, second: u32, third: u32, position: u32, fifth: u32, entry: *mut u8) -> u32;
    fn file_service_fs_delete(path: u32, mode: u32) -> u32;
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address, and the worker's row in the worker table.
struct StockPaths {
    header: u32,
    row: u32,
    payload: u32,
}

impl StockPaths {
    fn new(header: u32, row: u32) -> StockPaths {
        StockPaths { header, row, payload: read_word(header + 8) }
    }

    fn context_word(&self) -> u32 {
        read_word(CONTEXT)
    }
}

impl Port for StockPaths {
    fn word(&self, at: Offset) -> u32 {
        read_word(self.payload + at.0)
    }

    fn set_word(&mut self, at: Offset, value: u32) {
        write_word(self.payload + at.0, value)
    }

    fn byte(&self, at: Offset) -> u8 {
        read_byte(self.payload + at.0)
    }

    fn set_byte(&mut self, at: Offset, value: u8) {
        write_byte(self.payload + at.0, value)
    }

    fn address(&self, at: Offset) -> u32 {
        self.payload + at.0
    }

    fn context(&self) -> bool {
        self.context_word() != 0
    }

    fn cancelled(&self, cancel: Cancel) -> bool {
        let at = match cancel {
            Cancel::Filename => CONTEXT_FILENAME_CANCEL,
            Cancel::Metadata => CONTEXT_METADATA_CANCEL,
        };
        read_byte(self.context_word() + at) == 1
    }

    fn directory_record(&self, number: DirectoryNumber) -> DirectoryRecord {
        DirectoryRecord(read_word(self.context_word().wrapping_add(CONTEXT_DIRECTORIES).wrapping_add(number.0.wrapping_mul(4))))
    }

    fn refresh_media(&mut self) {
        unsafe { file_service_media_refresh(read_word(self.context_word() + CONTEXT_DEVICE), 0) }
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

    fn delete_file(&mut self) -> u32 {
        unsafe { file_service_delete_file_request(self.header) }
    }

    fn delete_directory(&mut self) -> u32 {
        unsafe { file_service_delete_directory_request(self.header, self.row) }
    }

    fn set_read_only(&mut self) -> u32 {
        unsafe { file_service_readonly_request(self.header) }
    }

    fn rebuild_catalog(&mut self, rebuild: Rebuild) -> RawStatus {
        let mode = match rebuild {
            Rebuild::Completed => 1,
            Rebuild::Failed => 0,
        };
        RawStatus(unsafe { file_service_catalog_rebuild(mode, 0) })
    }

    fn reload_catalog(&mut self) -> RawStatus {
        RawStatus(unsafe { file_service_rescan() })
    }

    fn split_path<const D: usize, const N: usize>(&mut self, path: u32, split: &mut Split<D, N>) -> RawStatus {
        RawStatus(unsafe {
            file_service_split_path(
                path,
                split.directory.as_mut_ptr(),
                split.name.as_mut_ptr(),
                &mut split.word,
                core::ptr::null_mut(),
                &mut split.flag,
            )
        })
    }

    fn wildcard(&mut self, path: u32) -> u32 {
        unsafe { file_service_path_wildcard(path) }
    }

    fn directory_number<const D: usize, const N: usize>(&mut self, split: &Split<D, N>) -> DirectoryNumber {
        DirectoryNumber(unsafe { file_service_directory_number(split.directory.as_ptr(), split.word) })
    }

    fn update_entry<const D: usize, const N: usize>(
        &mut self,
        service: EntryService,
        record: DirectoryRecord,
        split: &Split<D, N>,
        mode: u32,
        value: u32,
        wildcard: u32,
    ) -> RawStatus {
        let name = split.name.as_ptr();
        RawStatus(unsafe {
            match service {
                EntryService::Update => file_service_entry_update(record.0, name, mode, value, wildcard, 1),
                EntryService::UpdateOne => file_service_entry_update_one(record.0, name, mode, value, wildcard, 1),
            }
        })
    }

    fn device_state(&mut self, device: u32, state: &mut [u8; 48]) -> DeviceState {
        unsafe { file_service_device_state(device, state.as_mut_ptr()) };
        DeviceState(state[0])
    }

    fn file_number(&mut self, name: &[u8; 48]) -> FileNumber {
        FileNumber(unsafe { file_service_file_number(name.as_ptr()) })
    }

    fn extension_id(&mut self, extension: u32) -> ExtensionId {
        ExtensionId(unsafe { file_service_extension_key(extension) })
    }

    fn extension_kind(&mut self, id: ExtensionId) -> u32 {
        unsafe { file_service_extension_byte5(id.0) }
    }

    fn find_entry(&mut self, entry: &mut Entry) -> RawStatus {
        RawStatus(unsafe { file_service_catalog_find(0, 0, 0, u32::MAX, 0, entry.0.as_mut_ptr()) })
    }

    fn delete_path(&mut self, path: u32) -> RawStatus {
        RawStatus(unsafe { file_service_fs_delete(path, 0) })
    }
}

/// Entered at 0x7118fc by the media task for code 0x2c0a.
#[no_mangle]
pub extern "C" fn xm1_file_service_file_delete_worker(header: u32) -> u32 {
    paths::file_delete(&mut StockPaths::new(header, 0))
}

/// Entered at 0x7119b0 by the media task for code 0x2c03.
#[no_mangle]
pub extern "C" fn xm1_file_service_filename_set_worker(header: u32) -> u32 {
    paths::filename_set(&mut StockPaths::new(header, 0))
}

/// Entered at 0x711b50 by the media task for code 0x2c0b with its worker table row in r1.
#[no_mangle]
pub extern "C" fn xm1_file_service_directory_delete_worker(header: u32, row: u32) -> u32 {
    paths::directory_delete(&mut StockPaths::new(header, row))
}

/// Entered at 0x711bc8 by the media task for code 0x2c25.
#[no_mangle]
pub extern "C" fn xm1_file_service_image_remove_worker(header: u32) -> u32 {
    paths::image_remove(&mut StockPaths::new(header, 0))
}

/// Entered at 0x711ea4 by the media task for code 0x2c0c.
#[no_mangle]
pub extern "C" fn xm1_file_service_readonly_worker(header: u32) -> u32 {
    paths::read_only(&mut StockPaths::new(header, 0))
}

/// Entered at 0x711f58 by the media task for code 0x2c04.
#[no_mangle]
pub extern "C" fn xm1_file_service_attribute_set_worker(header: u32) -> u32 {
    paths::attribute_set(&mut StockPaths::new(header, 0))
}
