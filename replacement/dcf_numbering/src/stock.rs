//! Guest addresses, the retained services and the entries the veneers at the stock
//! addresses branch to.

use crate::numbering::{self, FolderScratch, Guest, FOLDER_ENTRY_BYTES, FOLDER_REQUEST_BYTES};
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

const CONTEXT_SLOT: u32 = 0x910204;
static DCIM: [u8; 6] = *b"\\DCIM\\";

extern "C" {
    fn dcf_media_status(selector: u32) -> u32;
    fn dcf_current_drive() -> u32;
    fn dcf_group_index(mask: u32) -> u32;
    fn dcf_report_event(kind: u32, code: u32);
    fn dcf_drive_prefix(destination: u32);
    fn dcf_copy_bytes(destination: u32, source: u32, length: u32);
    fn dcf_format_decimal(destination: u32, value: u32, digits: u32, pad: u32);
    fn dcf_configuration_byte(key: u32) -> u32;
    fn dcf_profile_directory(drive: u32, group: u32) -> u32;
    fn dcf_profile_file(drive: u32, group: u32) -> u32;
    fn dcf_store_profile_directory(drive: u32, group: u32, directory: u32, file: u32);
    fn dcf_store_profile_file(drive: u32, group: u32, file: u32);
    fn dcf_reset_profile();
    fn dcf_refresh_snapshot();
    fn dcf_fill_bytes(destination: u32, value: u32, length: u32);
    fn dcf_format_number(destination: u32, value: u32, digits: u32, pad: u32);
    fn dcf_copy_memory(destination: u32, source: u32, length: u32);
    fn dcf_open_directory(path: u32, entry: u32, flags: u32) -> u32;
    fn dcf_entry_cluster(entry: u32) -> u32;
    fn dcf_path_groups(mask: u32, path: u32);
    fn dcf_translate_status(request: u32, status: u32) -> u32;
}

const FOLDER_LIMIT_EVENT: (u32, u32) = (2, 0x51);
const PATH_TAIL_BYTES: u32 = 30;
const FOLDER_DIGITS: u32 = 3;
const FOLDER_SUFFIX_BYTES: u32 = 5;
const ZERO_PADDED: u32 = 1;

struct Stock;

impl Guest for Stock {
    fn read_u8(&self, address: u32) -> u8 {
        unsafe { read_volatile(address as *const u8) }
    }
    fn read_u16(&self, address: u32) -> u16 {
        unsafe { read_volatile(address as *const u16) }
    }
    fn read_u32(&self, address: u32) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }
    fn write_u8(&mut self, address: u32, value: u8) {
        unsafe { write_volatile(address as *mut u8, value) }
    }
    fn write_u16(&mut self, address: u32, value: u16) {
        unsafe { write_volatile(address as *mut u16, value) }
    }
    fn write_u32(&mut self, address: u32, value: u32) {
        unsafe { write_volatile(address as *mut u32, value) }
    }
    fn context(&self) -> u32 {
        self.read_u32(CONTEXT_SLOT)
    }
    fn dcim_text(&self) -> u32 {
        DCIM.as_ptr() as u32
    }
    fn media_status(&mut self) -> u32 {
        unsafe { dcf_media_status(0) }
    }
    fn current_drive(&mut self) -> u32 {
        unsafe { dcf_current_drive() }
    }
    fn group_index(&mut self, mask: u32) -> u32 {
        unsafe { dcf_group_index(mask) }
    }
    fn report_folder_limit(&mut self) {
        unsafe { dcf_report_event(FOLDER_LIMIT_EVENT.0, FOLDER_LIMIT_EVENT.1) }
    }
    fn drive_prefix(&mut self, destination: u32) {
        unsafe { dcf_drive_prefix(destination) }
    }
    fn copy_bytes(&mut self, destination: u32, source: u32, length: u32) {
        unsafe { dcf_copy_bytes(destination, source, length) }
    }
    fn format_decimal(&mut self, destination: u32, value: u32, digits: u32) {
        unsafe { dcf_format_decimal(destination, value, digits, ZERO_PADDED) }
    }
    fn configuration_byte(&mut self, key: u32) -> u32 {
        unsafe { dcf_configuration_byte(key) }
    }
    fn profile_directory(&mut self, drive: u32, group: u32) -> u32 {
        unsafe { dcf_profile_directory(drive, group) }
    }
    fn profile_file(&mut self, drive: u32, group: u32) -> u32 {
        unsafe { dcf_profile_file(drive, group) }
    }
    fn store_profile_directory(&mut self, drive: u32, group: u32, directory: u32, file: u32) {
        unsafe { dcf_store_profile_directory(drive, group, directory, file) }
    }
    fn store_profile_file(&mut self, drive: u32, group: u32, file: u32) {
        unsafe { dcf_store_profile_file(drive, group, file) }
    }
    fn reset_profile(&mut self) {
        unsafe { dcf_reset_profile() }
    }
    fn refresh_snapshot(&mut self) {
        unsafe { dcf_refresh_snapshot() }
    }
    fn clear_path_tail(&mut self, destination: u32) {
        unsafe { dcf_fill_bytes(destination, 0, PATH_TAIL_BYTES) }
    }
    fn format_folder_number(&mut self, destination: u32, number: u32) {
        unsafe { dcf_format_number(destination, number, FOLDER_DIGITS, ZERO_PADDED) }
    }
    fn copy_folder_suffix(&mut self, destination: u32, source: u32) {
        unsafe { dcf_copy_memory(destination, source, FOLDER_SUFFIX_BYTES) }
    }
    fn open_directory(&mut self, path: u32, entry: u32) -> u32 {
        unsafe { dcf_open_directory(path, entry, 0) }
    }
    fn entry_cluster(&mut self, entry: u32) -> u32 {
        unsafe { dcf_entry_cluster(entry) }
    }
    fn path_groups(&mut self, mask: u32, path: u32) {
        unsafe { dcf_path_groups(mask, path) }
    }
    fn translate_status(&mut self, status: u32) -> u32 {
        unsafe { dcf_translate_status(0, status) }
    }
}

#[repr(C, align(4))]
struct Scratch {
    request: [u8; FOLDER_REQUEST_BYTES as usize],
    entry: [u8; FOLDER_ENTRY_BYTES as usize],
    mask: u32,
}

/// Runs `body` with folder scratch in this frame; only the services and the guest
/// accessors touch it.
fn with_scratch<R>(body: impl FnOnce(FolderScratch) -> R) -> R {
    let mut space = MaybeUninit::<Scratch>::uninit();
    let base = space.as_mut_ptr() as usize as u32;
    body(FolderScratch {
        request: base,
        entry: base + FOLDER_REQUEST_BYTES,
        mask: base + FOLDER_REQUEST_BYTES + FOLDER_ENTRY_BYTES,
    })
}

#[no_mangle]
pub extern "C" fn xm1_dcf_set_prefix_mode(mode: u32) {
    numbering::set_prefix_mode(&mut Stock, mode)
}

#[no_mangle]
pub extern "C" fn xm1_dcf_check_name(name: u32, length: u32) -> u32 {
    numbering::check_name(&Stock, name, length).word()
}

#[no_mangle]
pub extern "C" fn xm1_dcf_apply_continuous_numbering(enabled: u32) {
    numbering::apply_continuous_numbering(&mut Stock, enabled)
}

#[no_mangle]
pub extern "C" fn xm1_dcf_read_numbers(directory: u32, file: u32) -> u32 {
    match numbering::read_numbers(&mut Stock) {
        Ok(pair) => {
            Stock.write_u16(directory, pair.directory);
            Stock.write_u16(file, pair.file);
            0
        }
        Err(status) => status.word(),
    }
}

#[no_mangle]
pub extern "C" fn xm1_dcf_prepare_name(path: u32, suffix: u32, prefix: u32) -> u32 {
    numbering::prepare_name(&mut Stock, path, suffix, prefix).word()
}

#[no_mangle]
pub extern "C" fn xm1_dcf_note_written(slot: u32) {
    with_scratch(|scratch| numbering::note_written(&mut Stock, slot, scratch))
}

#[no_mangle]
pub extern "C" fn xm1_dcf_read_profile(drive: u32, group: u32, directory: u32, file: u32) {
    let pair = numbering::read_profile(&mut Stock, drive, group);
    Stock.write_u16(directory, pair.directory);
    Stock.write_u16(file, pair.file);
}

#[no_mangle]
pub extern "C" fn xm1_dcf_write_profile(drive: u32, group: u32, directory: u32, file: u32) {
    numbering::write_profile(&mut Stock, drive, group, directory, file)
}

#[no_mangle]
pub extern "C" fn xm1_dcf_create_folder(number: u32, suffix: u32) -> u32 {
    with_scratch(|scratch| numbering::create_folder(&mut Stock, number, suffix, scratch))
}

#[no_mangle]
pub extern "C" fn xm1_dcf_last_used(drive: u32, group: u32, directory: u32, file: u32) -> u32 {
    let pair = numbering::last_used(&mut Stock, drive, group);
    Stock.write_u16(directory, pair.directory);
    Stock.write_u16(file, pair.file);
    0
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
