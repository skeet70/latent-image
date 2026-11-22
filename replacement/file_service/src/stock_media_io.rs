//! The stock boundary of the storage workers: the media-state context, the request header
//! the media task or the pattern pass 0x715b34 passes, and the retained volume, page,
//! filesystem and local-source services.

use crate::completion::RawStatus;
use crate::handles::FileIndex;
use crate::media_io::{self, Device, Direction, Geometry, LocalRecord, LocalStatus, Pages, Pattern, Port, Sectors, Step};
use crate::requests::Callback;
use crate::stock::{read_byte, read_half, read_word, write_word, ATTACH_STATE};
use crate::workers::Offset;

const CONTEXT: u32 = 0x910204;
const CONTEXT_DEVICE: u32 = 0x114;
const CONTEXT_CAPACITY: u32 = 0x16c50;
const GEOMETRY_BYTES: usize = 68;
const GEOMETRY_LOW: usize = 0x14;
const GEOMETRY_HIGH: usize = 0x18;
const QUERY_BYTES: usize = 68;

extern "C" {
    fn file_service_volume_geometry(device: u32, record: *mut u8);
    fn file_service_volume_flag(device: u32, flag: *mut u32) -> u32;
    fn file_service_volume_units(device: u32) -> u32;
    fn file_service_query_path(path: u32, output: *mut u8, flag: u32) -> u32;
    fn file_service_read_pages(device: u32, first: u32, count: u32, buffer: u32) -> u32;
    fn file_service_write_pages(device: u32, first: u32, count: u32, buffer: u32) -> u32;
    fn file_service_pattern_pass(header: u32, pattern: u32, step: u32) -> u32;
    fn file_service_fs_read(index: u32, buffer: u32, length: u32, raw: *mut u32) -> u32;
    fn file_service_local_record(record: *mut u32);
    fn file_service_local_size(handle: u32, size: *mut u32) -> u32;
    fn file_service_local_read(handle: u32, cursor: u32, buffer: u32, position: u32, length: u32) -> u32;
    fn file_service_local_position(position: u32);
}

/// A request a storage worker receives: the header address, whose word at +8 is the payload
/// address.
struct StockStorage {
    header: u32,
    payload: u32,
}

impl StockStorage {
    fn new(header: u32) -> StockStorage {
        StockStorage { header, payload: read_word(header + 8) }
    }
}

fn word_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

impl Port for StockStorage {
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

    fn store(&mut self, address: u32, value: u32) {
        write_word(address, value)
    }

    fn device(&self) -> Option<Device> {
        match read_word(CONTEXT) {
            0 => None,
            context => Some(Device(read_word(context + CONTEXT_DEVICE))),
        }
    }

    fn capacity(&self) -> Sectors {
        Sectors(read_word(read_word(CONTEXT) + CONTEXT_CAPACITY))
    }

    fn reset_attach_state(&mut self) {
        write_word(ATTACH_STATE, u32::MAX)
    }

    fn lookup(&mut self, tag: u32) -> FileIndex {
        FileIndex(crate::stock::xm1_file_service_map_lookup(tag))
    }

    fn translate(&mut self, raw: RawStatus) -> u32 {
        crate::stock_requests::xm1_file_service_translate(u32::from(read_half(self.header)), raw.0)
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

    fn geometry(&mut self, device: Device) -> Geometry {
        let mut record = [0u8; GEOMETRY_BYTES];
        unsafe { file_service_volume_geometry(device.0, record.as_mut_ptr()) };
        Geometry { low: word_at(&record, GEOMETRY_LOW), high: word_at(&record, GEOMETRY_HIGH) }
    }

    fn geometry_flag(&mut self, device: Device) -> (RawStatus, u32) {
        let mut flag = 0;
        let raw = unsafe { file_service_volume_flag(device.0, &mut flag) };
        (RawStatus(raw), flag)
    }

    fn geometry_units(&mut self, device: Device) -> u32 {
        unsafe { file_service_volume_units(device.0) }
    }

    fn query_path(&mut self, path: u32, flag: u8) -> u32 {
        let mut output = [0u8; QUERY_BYTES];
        unsafe { file_service_query_path(path, output.as_mut_ptr(), u32::from(flag)) }
    }

    fn transfer_pages(&mut self, direction: Direction, device: Device, pages: Pages) -> RawStatus {
        let transfer = match direction {
            Direction::Read => file_service_read_pages,
            Direction::Write => file_service_write_pages,
        };
        RawStatus(unsafe { transfer(device.0, pages.first.0, pages.count.0, pages.buffer) })
    }

    fn pattern_pass(&mut self, pattern: Pattern, step: Step) -> u32 {
        unsafe { file_service_pattern_pass(self.header, pattern.0, step.0) }
    }

    fn fs_read(&mut self, index: FileIndex, buffer: u32, length: u32) -> (u32, RawStatus) {
        let mut raw = 0;
        let count = unsafe { file_service_fs_read(index.0, buffer, length, &mut raw) };
        (count, RawStatus(raw))
    }

    fn local_record(&mut self) -> LocalRecord {
        let mut record = [0u32; 4];
        unsafe { file_service_local_record(record.as_mut_ptr()) };
        let [mode, handle, cursor, position] = record;
        LocalRecord { mode, handle, cursor, position }
    }

    fn local_size(&mut self, handle: u32) -> (LocalStatus, u32) {
        let mut size = [0u32; 2];
        let status = unsafe { file_service_local_size(handle, size.as_mut_ptr()) };
        (LocalStatus(status), size[1])
    }

    fn local_read(&mut self, record: &LocalRecord, buffer: u32, length: u32) -> LocalStatus {
        LocalStatus(unsafe { file_service_local_read(record.handle, record.cursor, buffer, record.position, length) })
    }

    fn local_position(&mut self, position: u32) {
        unsafe { file_service_local_position(position) }
    }
}

/// Entered at 0x715a58 by the media task for code 0x2c26.
#[no_mangle]
pub extern "C" fn xm1_file_service_capacity_worker(header: u32) -> u32 {
    media_io::capacity(&mut StockStorage::new(header))
}

/// Entered at 0x715de4 by the media task for code 0x2c2a.
#[no_mangle]
pub extern "C" fn xm1_file_service_pattern_worker(header: u32) -> u32 {
    media_io::pattern_test(&mut StockStorage::new(header))
}

/// Entered at 0x71ded4 by the media task for code 0x2c1e.
#[no_mangle]
pub extern "C" fn xm1_file_service_query_worker(header: u32) -> u32 {
    media_io::query_device(&mut StockStorage::new(header))
}

/// Entered at 0x71e828 by the media task for code 0x2c13 and by the `bl` at 0x715cd4 in the
/// pattern pass, which reads the returned status.
#[no_mangle]
pub extern "C" fn xm1_file_service_read_pages_worker(header: u32) -> u32 {
    media_io::transfer(&mut StockStorage::new(header), Direction::Read)
}

/// Entered at 0x71e8c4 by the media task for code 0x2c14 and by the `bl` at 0x715c30 in the
/// pattern pass, which reads the returned status.
#[no_mangle]
pub extern "C" fn xm1_file_service_write_pages_worker(header: u32) -> u32 {
    media_io::transfer(&mut StockStorage::new(header), Direction::Write)
}

/// Entered at 0x71edbc by the media task for code 0x2c18.
#[no_mangle]
pub extern "C" fn xm1_file_service_read_worker(header: u32) -> u32 {
    media_io::read(&mut StockStorage::new(header))
}
