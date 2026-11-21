//! The media task's storage workers: the capacity query (0x2c26), the pattern test (0x2c2a),
//! the device query (0x2c1e), the page read and write (0x2c13, 0x2c14) and the file read
//! (0x2c18). Offsets are into the payloads the posters build.

use crate::completion::RawStatus;
use crate::handles::FileIndex;
use crate::requests::Callback;
use crate::workers::Offset;

/// The device word of the active media-state context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Device(pub u32);

/// A sector count, the unit of the context's capacity and of a page transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sectors(pub u32);

/// The sectors a page transfer names and the buffer it moves them through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pages {
    pub first: Sectors,
    pub count: Sectors,
    pub buffer: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
}

/// The fill a pattern pass writes and reads back: 0xffff selects the bytes 0, 1, 2, ...;
/// any other word its low byte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pattern(pub u32);

/// The sectors a pattern pass advances between transfers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step(pub u32);

/// The words at +0x14 and +0x18 of the 68-byte record 0x701090 fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Geometry {
    pub low: u32,
    pub high: u32,
}

/// The 16-byte local-mode record at 0x8fc8a4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalRecord {
    pub mode: u32,
    pub handle: u32,
    pub cursor: u32,
    pub position: u32,
}

impl LocalRecord {
    /// Mode 1 serves reads from the local source instead of the card.
    pub fn redirects(&self) -> bool {
        self.mode == 1
    }
}

/// A local-source call's result; nonzero refuses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalStatus(pub u32);

const CALLBACK: Offset = Offset(0);

mod pages {
    use super::Offset;
    pub const FIRST: Offset = Offset(4);
    pub const COUNT: Offset = Offset(8);
    pub const BUFFER: Offset = Offset(0xc);
}

mod pattern {
    use super::Offset;
    pub const REPEATS: Offset = Offset(8);
}

mod query {
    use super::Offset;
    pub const PATH: Offset = Offset(4);
    pub const FLAG: Offset = Offset(0x24);
}

mod read {
    use super::Offset;
    pub const HANDLE: Offset = Offset(4);
    pub const LENGTH: Offset = Offset(8);
    pub const BUFFER: Offset = Offset(0xc);
    pub const FLAG: Offset = Offset(0x10);
    pub const STATUS: Offset = Offset(0x14);
    pub const COUNT: Offset = Offset(0x18);
}

const RAW_NO_CONTEXT: RawStatus = RawStatus(2);
const RAW_OUT_OF_RANGE: RawStatus = RawStatus(19);
/// The pattern test's raw status without a context.
const RAW_PATTERN_NO_CONTEXT: RawStatus = RawStatus(5);
const QUERY_FAILED: u32 = 11;
const LOCAL_FAILED: u32 = 5;
/// Units of 0x40000 bytes below this count are reported in bytes; the rest as all ones.
const UNIT_LIMIT: u32 = 0x4000;
const UNIT_SHIFT: u32 = 18;
const UNKNOWN_SIZE: u32 = u32::MAX;

/// The six passes of one pattern-test repeat, in stock's order.
const PASSES: [(Pattern, Step); 6] = [
    (Pattern(0x55), Step(1)),
    (Pattern(0xaa), Step(1)),
    (Pattern(0xffff), Step(1)),
    (Pattern(0x55), Step(256)),
    (Pattern(0xaa), Step(256)),
    (Pattern(0xffff), Step(256)),
];

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    fn set_word(&mut self, at: Offset, value: u32);
    fn byte(&self, at: Offset) -> u8;
    /// The guest address of the payload bytes at `at`.
    fn address(&self, at: Offset) -> u32;
    /// Stores to a caller's completion word.
    fn store(&mut self, address: u32, value: u32);

    /// The active media-state context's device; none without a context.
    fn device(&self) -> Option<Device>;
    fn capacity(&self) -> Sectors;
    /// Sets the word at 0x910208 to all ones.
    fn reset_page_state(&mut self);

    fn lookup(&mut self, tag: u32) -> FileIndex;
    /// Translates a raw result, reporting it when the translation reports.
    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete(&mut self, callback: Callback, status: u32, value: u32);
    fn complete_status(&mut self, callback: Callback, status: u32);
    /// Signals the blocking caller's completion.
    fn done(&mut self);

    fn geometry(&mut self, device: Device) -> Geometry;
    /// 0x7012a0's result and the flag word it writes.
    fn geometry_flag(&mut self, device: Device) -> (RawStatus, u32);
    /// 0x7012c0's count of 0x40000-byte units.
    fn geometry_units(&mut self, device: Device) -> u32;
    /// 0x701e18 over the path and flag byte; nonzero fails.
    fn query_path(&mut self, path: u32, flag: u8) -> u32;
    fn transfer_pages(&mut self, direction: Direction, device: Device, pages: Pages) -> RawStatus;
    /// One retained pass 0x715b34 over this request; returns its status.
    fn pattern_pass(&mut self, pattern: Pattern, step: Step) -> u32;
    /// Reads `length` bytes into `buffer`; returns the count read.
    fn fs_read(&mut self, index: FileIndex, buffer: u32, length: u32) -> (u32, RawStatus);
    fn local_record(&mut self) -> LocalRecord;
    /// 0x6aa4ec's result and the source size it writes.
    fn local_size(&mut self, handle: u32) -> (LocalStatus, u32);
    fn local_read(&mut self, record: &LocalRecord, buffer: u32, length: u32) -> LocalStatus;
    fn local_position(&mut self, position: u32);
}

/// The capacity query at 0x715a58: the callback receives the status and the size; the
/// worker returns 0.
pub fn capacity<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let (raw, size) = match port.device() {
        None => (RAW_NO_CONTEXT, 0),
        Some(device) => {
            let geometry = port.geometry(device);
            let (raw, flag) = port.geometry_flag(device);
            if raw != RawStatus(0) {
                (raw, 0)
            } else if flag != 0 && !cfg!(file_service_control_capacity_ignores_units) {
                let units = port.geometry_units(device);
                (raw, if units < UNIT_LIMIT { units << UNIT_SHIFT } else { UNKNOWN_SIZE })
            } else {
                (raw, if geometry.high == 0 { geometry.low } else { UNKNOWN_SIZE })
            }
        }
    };
    let status = port.translate(raw);
    port.complete(callback, status, size);
    0
}

/// The pattern test at 0x715de4: the payload's count of repeats of the six passes, stopping
/// at the first nonzero pass status.
pub fn pattern_test<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let repeats = if cfg!(file_service_control_pattern_single_repeat) {
        port.word(pattern::REPEATS).min(1)
    } else {
        port.word(pattern::REPEATS)
    };
    let raw = if port.device().is_none() { RAW_PATTERN_NO_CONTEXT } else { RawStatus(passes(port, repeats)) };
    let status = port.translate(raw);
    port.complete_status(callback, status);
    status
}

fn passes<P: Port>(port: &mut P, repeats: u32) -> u32 {
    for _ in 0..repeats {
        for (pattern, step) in PASSES {
            let status = port.pattern_pass(pattern, step);
            if status != 0 {
                return status;
            }
        }
    }
    0
}

/// The device query at 0x71ded4; it reads no context.
pub fn query_device<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let result = port.query_path(port.address(query::PATH), port.byte(query::FLAG));
    let status = if result == 0 {
        0
    } else if cfg!(file_service_control_query_raw_status) {
        result
    } else {
        QUERY_FAILED
    };
    port.complete_status(callback, status);
    status
}

/// The page read at 0x71e828 and write at 0x71e8c4. A range past the capacity or of no
/// sectors refuses with 19.
pub fn transfer<P: Port>(port: &mut P, direction: Direction) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let raw = match port.device() {
        None => RAW_NO_CONTEXT,
        Some(device) => {
            let pages = Pages {
                first: Sectors(port.word(pages::FIRST)),
                count: Sectors(port.word(pages::COUNT)),
                buffer: port.word(pages::BUFFER),
            };
            let end = Sectors(pages.count.0.wrapping_add(pages.first.0));
            let past = if cfg!(file_service_control_pages_range_inclusive) {
                end >= port.capacity()
            } else {
                end > port.capacity()
            };
            if past || pages.count == Sectors(0) {
                RAW_OUT_OF_RANGE
            } else {
                port.transfer_pages(direction, device, pages)
            }
        }
    };
    let status = port.translate(raw);
    if !cfg!(file_service_control_pages_keep_state) {
        port.reset_page_state();
    }
    port.complete_status(callback, status);
    status
}

/// The file read at 0x71edbc. In local mode it reads from the local source, delivers the
/// untranslated status and the length to the callback, and signals no blocking caller.
pub fn read<P: Port>(port: &mut P) -> u32 {
    let index = port.lookup(port.word(read::HANDLE));
    port.set_word(read::HANDLE, index.0);
    let callback = Callback(port.word(CALLBACK));
    let record = port.local_record();
    if record.redirects() {
        let (status, length) = read_local(port, &record);
        port.complete(callback, status, length);
        return status;
    }
    let (count, raw) = if port.device().is_some() {
        port.fs_read(index, port.word(read::BUFFER), port.word(read::LENGTH))
    } else {
        (0, RAW_NO_CONTEXT)
    };
    let status = port.translate(raw);
    port.complete(callback, status, count);
    if port.word(read::FLAG) != 0 {
        let at = port.word(read::STATUS);
        if at != 0 {
            port.store(at, status);
        }
        let at = port.word(read::COUNT);
        if at != 0 {
            port.store(at, count);
        }
        port.done();
    }
    status
}

/// Reads at the record's position into the buffer rounded up to even, clipped to the
/// source size, and advances the position.
fn read_local<P: Port>(port: &mut P, record: &LocalRecord) -> (u32, u32) {
    let buffer = port.word(read::BUFFER);
    let length = port.word(read::LENGTH);
    let (refused, size) = port.local_size(record.handle);
    if refused != LocalStatus(0) {
        return (LOCAL_FAILED, length);
    }
    let buffer = buffer + (buffer & 1);
    let length = if record.position.wrapping_add(length) > size { size.wrapping_sub(record.position) } else { length };
    if port.local_read(record, buffer, length) != LocalStatus(0) {
        return (LOCAL_FAILED, length);
    }
    if !cfg!(file_service_control_read_redirect_keeps_position) {
        port.local_position(length.wrapping_add(record.position));
    }
    (0, length)
}
