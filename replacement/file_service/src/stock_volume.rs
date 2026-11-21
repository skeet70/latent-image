//! The stock boundary of the volume workers: the request header the media task passes, the
//! media-state words at 0x910204 and 0x910208, and the retained volume, format and memory
//! services.

use crate::requests::Callback;
use crate::stock::{file_service_copy_memory, file_service_fill_memory, read_byte, read_word, write_byte, write_half, write_word};
use crate::volume::{self, Context, Field, Letter, Port, Residue, Volume};
use crate::workers::Offset;

const CONTEXT: u32 = 0x910204;
const ATTACH_STATE: u32 = 0x910208;
const DEFAULTS: u32 = 0x814558;
const DEFAULTS_BYTES: u32 = 216;

extern "C" {
    fn file_service_device_for_letter(letter: u32) -> u32;
    fn file_service_volume_record(volume: u32) -> u32;
    fn file_service_volume_release(volume: u32) -> u32;
    fn file_service_volume_switch(from: u32, to: u32) -> u32;
    fn file_service_volume_other(volume: u32) -> u32;
    fn file_service_volume_attach(header: u32) -> u32;
    fn file_service_volume_reattach(header: u32) -> u32;
    fn file_service_volume_detach(header: u32) -> u32;
    fn file_service_volume_format(header: u32, volume: u32) -> u32;
    fn file_service_volume_reformat(header: u32, volume: u32) -> u32;
    fn file_service_media_snapshot();
    fn file_service_set_level(slot: u32, level: u32);
    fn file_service_flagged_prepare();
    fn file_service_catalog_rebuild(first: u32, second: u32) -> u32;
    fn file_service_flagged_select(first: u32, second: u32) -> u32;
    fn file_service_no_volume_status() -> u32;
}

/// A request the media task hands a worker: the header address, whose word at +8 is the
/// payload address.
struct StockVolume {
    header: u32,
    payload: u32,
}

impl StockVolume {
    fn new(header: u32) -> StockVolume {
        StockVolume { header, payload: read_word(header + 8) }
    }
}

impl Port for StockVolume {
    fn word(&self, at: Offset) -> u32 {
        read_word(self.payload + at.0)
    }

    fn set_word(&mut self, at: Offset, value: u32) {
        write_word(self.payload + at.0, value)
    }

    fn byte(&self, at: Offset) -> u8 {
        read_byte(self.payload + at.0)
    }

    fn context(&self) -> Context {
        Context(read_word(CONTEXT))
    }

    fn set_context(&mut self, context: Context) {
        write_word(CONTEXT, context.0)
    }

    fn attach_state(&self) -> u32 {
        read_word(ATTACH_STATE)
    }

    fn field(&self, context: Context, at: Field) -> u32 {
        read_word(context.0.wrapping_add(at.0))
    }

    fn field_byte(&self, context: Context, at: Field) -> u8 {
        read_byte(context.0.wrapping_add(at.0))
    }

    fn set_field(&mut self, context: Context, at: Field, value: u32) {
        write_word(context.0.wrapping_add(at.0), value)
    }

    fn set_field_half(&mut self, context: Context, at: Field, value: u16) {
        write_half(context.0.wrapping_add(at.0), value)
    }

    fn set_field_byte(&mut self, context: Context, at: Field, value: u8) {
        write_byte(context.0.wrapping_add(at.0), value)
    }

    fn complete(&mut self, callback: Callback, value: u32) {
        if callback.0 != 0 {
            let function: extern "C" fn(u32) = unsafe { core::mem::transmute(callback.0 as usize) };
            function(value)
        }
    }

    fn device_for_letter(&mut self, letter: Letter) -> Volume {
        Volume(unsafe { file_service_device_for_letter(u32::from(letter.0)) })
    }

    fn volume_record(&mut self, volume: Volume) -> u32 {
        unsafe { file_service_volume_record(volume.0) }
    }

    fn release_volume(&mut self, volume: Volume) -> Residue {
        Residue(unsafe { file_service_volume_release(volume.0) })
    }

    fn switch_volume(&mut self, from: Volume, to: Volume) -> Residue {
        Residue(unsafe { file_service_volume_switch(from.0, to.0) })
    }

    fn other_volume(&mut self, volume: Volume) -> Volume {
        Volume(unsafe { file_service_volume_other(volume.0) })
    }

    fn attach(&mut self) -> Residue {
        Residue(unsafe { file_service_volume_attach(self.header) })
    }

    fn reattach(&mut self) -> Residue {
        Residue(unsafe { file_service_volume_reattach(self.header) })
    }

    fn detach(&mut self) -> Residue {
        Residue(unsafe { file_service_volume_detach(self.header) })
    }

    fn format(&mut self, volume: Volume) -> u32 {
        unsafe { file_service_volume_format(self.header, volume.0) }
    }

    fn reformat(&mut self, volume: Volume) -> u32 {
        unsafe { file_service_volume_reformat(self.header, volume.0) }
    }

    fn copy_defaults(&mut self, context: Context, at: Field) -> Residue {
        Residue(unsafe { file_service_copy_memory(context.0.wrapping_add(at.0), DEFAULTS, DEFAULTS_BYTES) })
    }

    fn fill_zero(&mut self, context: Context, at: Field, bytes: u32) {
        unsafe { file_service_fill_memory(context.0.wrapping_add(at.0), 0, bytes) }
    }

    fn snapshot_media_state(&mut self) {
        unsafe { file_service_media_snapshot() }
    }

    fn set_level(&mut self, slot: u32, level: u32) {
        unsafe { file_service_set_level(slot, level) }
    }

    fn flagged_prepare(&mut self) {
        unsafe { file_service_flagged_prepare() }
    }

    fn catalog_rebuild(&mut self, first: u32, second: u32) -> Residue {
        Residue(unsafe { file_service_catalog_rebuild(first, second) })
    }

    fn flagged_select(&mut self, first: u32, second: u32) -> Residue {
        Residue(unsafe { file_service_flagged_select(first, second) })
    }

    fn no_volume_status(&mut self) -> Residue {
        Residue(unsafe { file_service_no_volume_status() })
    }
}

/// Entered at 0x7120d8 by the media task for code 0x2c00.
#[no_mangle]
pub extern "C" fn xm1_file_service_volume_attach_worker(header: u32) -> u32 {
    volume::attach(&mut StockVolume::new(header))
}

/// Entered at 0x7122dc by the media task for code 0x2c01.
#[no_mangle]
pub extern "C" fn xm1_file_service_volume_detach_worker(header: u32) -> u32 {
    volume::detach_all(&mut StockVolume::new(header))
}

/// Entered at 0x7123b4 by the media task for code 0x2c11.
#[no_mangle]
pub extern "C" fn xm1_file_service_volume_format_armed_worker(header: u32) -> u32 {
    volume::format_armed(&mut StockVolume::new(header))
}

/// Entered at 0x712580 by the media task for code 0x2c12.
#[no_mangle]
pub extern "C" fn xm1_file_service_volume_format_worker(header: u32) -> u32 {
    volume::format(&mut StockVolume::new(header))
}
