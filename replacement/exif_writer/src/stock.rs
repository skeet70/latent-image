//! The retained services and the entries the veneers at the stock addresses branch to.

use crate::writer::{self, callback, Guest, Sources};
use core::mem::MaybeUninit;
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn exif_copy_bytes(destination: u32, source: u32, length: u32);
    fn exif_copy_memory(destination: u32, source: u32, length: u32);
    fn exif_copy_string(destination: u32, source: u32);
    fn exif_copy_string_bounded(destination: u32, source: u32, length: u32);
    fn exif_string_length(source: u32) -> u32;
    fn exif_fill(destination: u32, value: u32, length: u32);
}

type RuleCallback = extern "C" fn(u32, u32, u32, u32, u32);

struct Stock;

impl Guest for Stock {
    fn r8(&self, address: u32) -> u8 {
        unsafe { read_volatile(address as *const u8) }
    }
    fn r16(&self, address: u32) -> u16 {
        unsafe { read_volatile(address as *const u16) }
    }
    fn r32(&self, address: u32) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }
    fn w8(&mut self, address: u32, value: u8) {
        unsafe { write_volatile(address as *mut u8, value) }
    }
    fn w16(&mut self, address: u32, value: u16) {
        unsafe { write_volatile(address as *mut u16, value) }
    }
    fn w32(&mut self, address: u32, value: u32) {
        unsafe { write_volatile(address as *mut u32, value) }
    }
    fn copy_bytes(&mut self, destination: u32, source: u32, length: u32) {
        unsafe { exif_copy_bytes(destination, source, length) }
    }
    fn copy_memory(&mut self, destination: u32, source: u32, length: u32) {
        unsafe { exif_copy_memory(destination, source, length) }
    }
    fn copy_string(&mut self, destination: u32, source: u32) {
        unsafe { exif_copy_string(destination, source) }
    }
    fn copy_string_bounded(&mut self, destination: u32, source: u32, length: u32) {
        unsafe { exif_copy_string_bounded(destination, source, length) }
    }
    fn string_length(&mut self, source: u32) -> u32 {
        unsafe { exif_string_length(source) }
    }
    fn fill(&mut self, destination: u32, value: u32, length: u32) {
        unsafe { exif_fill(destination, value, length) }
    }
    fn rule_callback(&mut self, callback: u32, source: u32, slot: u32, cursor: u32, layout: u32, entry: u32) {
        let function: RuleCallback = unsafe { core::mem::transmute(callback as usize) };
        function(source, slot, cursor, layout, entry)
    }
    fn frame<R>(&mut self, body: impl FnOnce(&mut Self, u32) -> R) -> R {
        let mut space = MaybeUninit::<[u32; 16]>::uninit();
        body(self, space.as_mut_ptr() as usize as u32)
    }
}

#[no_mangle]
pub extern "C" fn xm1_exif_initialize(mode: u32, order: u32, directory: u32, buffer: u32, option: u32) {
    writer::initialize(&mut Stock, mode as u16, order as u16, directory as u16, buffer, option as u16)
}

#[no_mangle]
pub extern "C" fn xm1_exif_write_primary(source: u32, destination: u32) -> u32 {
    writer::write_primary(&mut Stock, source, destination)
}

#[no_mangle]
pub extern "C" fn xm1_exif_write_exif(source: u32, destination: u32) -> u32 {
    writer::write_exif(&mut Stock, source, destination)
}

#[no_mangle]
pub extern "C" fn xm1_exif_write_makernote(source: u32, destination: u32) -> u32 {
    writer::write_makernote(&mut Stock, source, destination)
}

#[no_mangle]
pub extern "C" fn xm1_exif_write_gps(source: u32, destination: u32) -> u32 {
    writer::write_gps(&mut Stock, source, destination)
}

#[no_mangle]
pub extern "C" fn xm1_exif_patch_thumbnail_length(length: u32) {
    writer::patch_thumbnail_length(&mut Stock, length)
}

#[no_mangle]
pub extern "C" fn xm1_exif_transfer_saved_parser(parser: u32, bank: u32, direction: u32) -> i32 {
    writer::transfer_saved_parser(&mut Stock, parser, bank, direction)
}

#[no_mangle]
pub extern "C" fn xm1_exif_swap_halfword(address: u32) {
    writer::swap_halfword(&mut Stock, address)
}

#[no_mangle]
pub extern "C" fn xm1_exif_copy_value(destination: u32, source: u32, length: u32) -> u32 {
    writer::copy_value(&mut Stock, destination, source, length)
}

#[no_mangle]
pub extern "C" fn xm1_exif_type_size(value_type: u32) -> u32 {
    writer::type_size(&Stock, value_type)
}

#[no_mangle]
pub extern "C" fn xm1_exif_saved_directory_slots(kind: u32, parser: u32) -> u32 {
    writer::saved_directory_slots(&Stock, kind, parser)
}

#[no_mangle]
pub extern "C" fn xm1_exif_saved_directory_values(kind: u32, parser: u32) -> u32 {
    writer::saved_directory_values(&Stock, kind, parser)
}

#[no_mangle]
pub extern "C" fn xm1_exif_select_table(kind: u32) -> u32 {
    writer::select_table(kind)
}

#[no_mangle]
pub extern "C" fn xm1_exif_table_count(kind: u32) -> u32 {
    writer::table_count(kind)
}

#[no_mangle]
pub extern "C" fn xm1_exif_write_filtered_directory(kind: u32, source: u32, destination: u32, base: u32) -> u32 {
    writer::write_filtered_directory(&mut Stock, kind, source, destination, base)
}

#[no_mangle]
pub extern "C" fn xm1_exif_write_exif_header(output: u32, primary: u32, exif: u32, maker: u32, gps: u32, thumbnail: u32, directory: u32, option: u32) -> u32 {
    let sources = Sources { primary, exif, maker, gps, thumbnail };
    writer::write_exif_header(&mut Stock, output, sources, directory as u16, option)
}

#[no_mangle]
pub extern "C" fn xm1_exif_finish(total: u32, thumbnail_length: u32) {
    writer::finish_exif(&mut Stock, total, thumbnail_length)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_thumbnail_offset(_: u32, slot: u32, _: u32, _: u32, entry: u32) {
    callback::thumbnail_offset(&mut Stock, slot, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_thumbnail_length(_: u32, slot: u32, _: u32, _: u32, entry: u32) {
    callback::thumbnail_length(&mut Stock, slot, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_exif_pointer(_: u32, slot: u32, _: u32, _: u32, entry: u32) {
    callback::exif_pointer(&mut Stock, slot, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_gps_pointer(_: u32, slot: u32, _: u32, _: u32, entry: u32) {
    callback::gps_pointer(&mut Stock, slot, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_one(_: u32, _: u32, _: u32, _: u32, entry: u32) {
    callback::one(&mut Stock, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_makernote(_: u32, slot: u32, _: u32, _: u32, entry: u32) {
    callback::makernote(&mut Stock, slot, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_interoperability_pointer(_: u32, slot: u32, _: u32, _: u32, entry: u32) {
    callback::interoperability_pointer(&mut Stock, slot, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_packed_halves(source: u32, _: u32, _: u32, _: u32, entry: u32) {
    callback::packed_halves(&mut Stock, source, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_source_word(source: u32, _: u32, _: u32, _: u32, entry: u32) {
    callback::source_word(&mut Stock, source, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_staged_block(source: u32, _: u32, cursor: u32, layout: u32, entry: u32) {
    callback::staged_block(&mut Stock, source, cursor, layout, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_source_bytes_256(source: u32, _: u32, cursor: u32, layout: u32, entry: u32) {
    callback::source_bytes(&mut Stock, source, cursor, layout, entry, 256)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_source_bytes_122(source: u32, _: u32, cursor: u32, layout: u32, entry: u32) {
    callback::source_bytes(&mut Stock, source, cursor, layout, entry, 122)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_default_text(source: u32, _: u32, cursor: u32, layout: u32, entry: u32) {
    callback::default_text(&mut Stock, source, cursor, layout, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_copyright(source: u32, _: u32, cursor: u32, layout: u32, entry: u32) {
    callback::copyright(&mut Stock, source, cursor, layout, entry)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_printim(source: u32, slot: u32, cursor: u32, _: u32, _: u32) -> u32 {
    callback::printim(&mut Stock, source, slot, cursor)
}

#[no_mangle]
pub extern "C" fn xm1_exif_cb_image_records(source: u32, _: u32, cursor: u32, _: u32, entry: u32) {
    callback::image_records(&mut Stock, source, cursor, entry)
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
