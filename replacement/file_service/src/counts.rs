//! The media task's catalog-count workers for codes 0x2c0e and 0x2c0f. Each parses the posted
//! image path, walks the reserved pages of a directory window for the page whose cluster's
//! path matches the parsed prefix, finds the file's position there through 0x7189b4 and stores
//! counts derived from it to the poster's words.

use crate::completion::RawStatus;
use crate::requests::Callback;

/// A byte offset into a posted request payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset(pub u32);

/// A directory number, the low half of 0x712dac's result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Directory(pub u16);

/// A reserved page number of a directory window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Page(pub u32);

/// The word 0x713264 returns for an extension: zero for none, else the key word of the
/// extension's record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionKey(pub u32);

/// The four-digit number 0x713360 parses from the 8.3 pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileNumber(pub u32);

/// The cluster 0x713d60 gives for a reserved page; zero for none.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Cluster(pub u32);

/// The context's window pairs of first and last reserved page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// Context words 0x2178 and 0x217c, used when 0x71543c sets its marker.
    Marked,
    /// Context words 0x218c and 0x2190.
    Unmarked,
}

/// The working storage the path services fill, in stock's frame order from fp-0x80.
#[repr(C)]
pub struct Frame {
    /// The 8.3 pattern 0x71543c writes; its extension starts at byte 8.
    pub pattern: [u8; 12],
    pub directory: [u8; 16],
    pub query: [u8; 32],
    pub prefix: [u8; 32],
    pub marker: u32,
    pub secondary: u32,
    pub cluster: Cluster,
    pub position: u32,
}

impl Default for Frame {
    fn default() -> Frame {
        Frame { pattern: [0; 12], directory: [0; 16], query: [0; 32], prefix: [0; 32], marker: 0, secondary: 0, cluster: Cluster(0), position: 0 }
    }
}

/// The fields of the lookup record 0x7189b4 takes that vary per call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lookup {
    pub cluster: Cluster,
    /// Whether the directory's context entry was neither 0 nor -1.
    pub listed: bool,
    pub page: Page,
}

const CALLBACK: Offset = Offset(0);
const FIRST_COUNT: Offset = Offset(0x34);
const SECOND_COUNT: Offset = Offset(0x38);

const RAW_NO_CONTEXT: RawStatus = RawStatus(2);
const RAW_UNSUPPORTED: RawStatus = RawStatus(4);
const RAW_NOT_FOUND: RawStatus = RawStatus(7);
const FIRST_NUMBERED: u16 = 100;
const UNLISTED: u32 = 0xffff_ffff;

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    /// The guest address of the payload bytes at `at`.
    fn address(&self, at: Offset) -> u32;
    fn store(&mut self, address: u32, value: u32);

    /// Whether a media-state context is active.
    fn context(&self) -> bool;
    /// The context word at 0x2604 + 4 * directory.
    fn directory_entry(&self, directory: Directory) -> u32;
    fn window(&self, window: Window) -> (Page, Page);
    /// The context byte at 0x12fa6 + number.
    fn group(&self, number: FileNumber) -> u8;
    /// The context half at 0x1577e + 2 * group.
    fn group_start(&self, group: u8) -> FileNumber;
    /// The context byte at 0x15846 + index.
    fn page_flags(&self, index: u32) -> u8;
    /// The context half at 0x127d6 + 2 * (page - 100).
    fn page_files(&self, page: Page) -> u16;
    /// The context half at 0x35a4 + 2 * (directory - 100).
    fn directory_files(&self, directory: Directory) -> u16;

    /// 0x70e664 with value 0 over 32 bytes.
    fn clear(&mut self, buffer: &mut [u8; 32]);
    fn split_path(&mut self, path: u32, frame: &mut Frame) -> RawStatus;
    fn extension_key(&mut self, frame: &Frame) -> ExtensionKey;
    /// 0x713508: byte 4 of the extension record.
    fn extension_byte4(&mut self, key: ExtensionKey) -> u32;
    /// 0x713488: byte 5 of the extension record.
    fn extension_byte5(&mut self, key: ExtensionKey) -> u32;
    fn directory_number(&mut self, frame: &Frame) -> u32;
    fn reserved_cluster(&mut self, page: Page, frame: &mut Frame) -> RawStatus;
    /// 0x713db8 into the frame's query; zero when it found the cluster's path.
    fn cluster_path(&mut self, frame: &mut Frame) -> u32;
    /// 0x70e884 over the frame's query and prefix; zero when equal.
    fn text_difference(&mut self, frame: &Frame) -> u32;
    fn directory_position(&mut self, lookup: Lookup, frame: &mut Frame) -> RawStatus;
    fn file_number(&mut self, frame: &Frame) -> FileNumber;

    /// Translates a raw filesystem result, reporting it when the translation reports.
    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete_status(&mut self, callback: Callback, status: u32);
}

/// The worker; `Counts` (0x2c0f) also refuses an extension whose record byte 5 is set.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Position,
    Counts,
}

struct Found {
    directory: Directory,
    page: Page,
}

fn listed(entry: u32) -> bool {
    entry != 0 && entry != UNLISTED
}

fn locate(port: &mut impl Port, frame: &mut Frame, kind: Kind) -> Result<Found, RawStatus> {
    if !port.context() {
        return Err(RAW_NO_CONTEXT);
    }
    port.clear(&mut frame.prefix);
    let path = port.address(Offset(4));
    let split = port.split_path(path, frame);
    if split.0 != 0 {
        return Err(split);
    }
    let key = port.extension_key(frame);
    if key.0 == 0 || port.extension_byte4(key) == 0 {
        return Err(RAW_UNSUPPORTED);
    }
    let checks_byte5 = kind == Kind::Counts && !cfg!(file_service_control_counts_extension_unchecked);
    if checks_byte5 && port.extension_byte5(key) != 0 {
        return Err(RAW_UNSUPPORTED);
    }
    let directory = Directory(port.directory_number(frame) as u16);
    let (first, last, in_window) = if directory.0 >= FIRST_NUMBERED && listed(port.directory_entry(directory)) {
        (Page(u32::from(directory.0)), Page(u32::from(directory.0)), true)
    } else {
        let (window, in_window) = if frame.marker != 0 {
            (Window::Marked, false)
        } else {
            (Window::Unmarked, directory.0 != 0 && listed(port.directory_entry(directory)))
        };
        let (first, last) = port.window(window);
        if first > last {
            return Err(RAW_NOT_FOUND);
        }
        (first, last, in_window)
    };
    let mut page = first;
    let mut raw;
    loop {
        raw = port.reserved_cluster(page, frame);
        if raw.0 == 0 && frame.cluster.0 != 0 {
            port.clear(&mut frame.query);
            if port.cluster_path(frame) == 0 && port.text_difference(frame) == 0 {
                let lookup = Lookup { cluster: frame.cluster, listed: in_window, page };
                let found = port.directory_position(lookup, frame);
                return if found.0 == 0 { Ok(Found { directory, page }) } else { Err(found) };
            }
        }
        page = Page(page.0.wrapping_add(1));
        if page > last {
            break;
        }
    }
    Err(if raw.0 == 0 { RAW_NOT_FOUND } else { raw })
}

/// Numbers from the group's start up to `number` that share its group.
fn group_rank(port: &impl Port, number: FileNumber) -> u32 {
    let group = port.group(number);
    if group == 0 {
        return 0;
    }
    let mut candidate = port.group_start(group);
    let mut rank = 0u32;
    while candidate != number {
        if port.group(candidate) == group {
            rank = rank.wrapping_add(1);
        }
        candidate = FileNumber(candidate.0.wrapping_add(1));
    }
    rank
}

/// Flag bytes 0..max(position, 1) that are zero or odd, less one, plus the file counts of
/// pages 100..page.
fn folded_count(port: &impl Port, position: u32, page: Page) -> u32 {
    let mut count = 0u32.wrapping_sub(1);
    let mut index = 0u32;
    loop {
        let flags = port.page_flags(index);
        if flags == 0 || flags & 1 != 0 {
            count = count.wrapping_add(1);
        }
        index = index.wrapping_add(1);
        let more = if cfg!(file_service_control_counts_flags_short) { position > index } else { position >= index };
        if !more {
            break;
        }
    }
    if page.0 > u32::from(FIRST_NUMBERED) {
        let mut earlier = u32::from(FIRST_NUMBERED);
        while earlier != page.0 {
            count = count.wrapping_add(u32::from(port.page_files(Page(earlier))));
            earlier += 1;
        }
    }
    count
}

fn complete(port: &mut impl Port, callback: Callback, raw: RawStatus) -> u32 {
    let status = port.translate(raw);
    port.complete_status(callback, status);
    status
}

/// Stores the file's position in its directory plus the file counts of directories
/// 100..directory to the word at payload +0x34, and returns the translated status.
pub fn file_position(port: &mut impl Port) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let mut frame = Frame::default();
    let raw = match locate(port, &mut frame, Kind::Position) {
        Ok(found) => {
            let mut position = frame.position;
            if found.directory.0 > FIRST_NUMBERED && !cfg!(file_service_control_position_without_directories) {
                for earlier in FIRST_NUMBERED..found.directory.0 {
                    position = position.wrapping_add(u32::from(port.directory_files(Directory(earlier))));
                }
            }
            port.store(port.word(FIRST_COUNT), position);
            RawStatus(0)
        }
        Err(raw) => raw,
    };
    complete(port, callback, raw)
}

/// Stores the file's rank within its number group to the word at payload +0x38 and the
/// folded count to the word at payload +0x34; returns 0.
pub fn file_counts(port: &mut impl Port) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let mut frame = Frame::default();
    let raw = match locate(port, &mut frame, Kind::Counts) {
        Ok(found) => {
            let number = port.file_number(&frame);
            let rank = group_rank(port, number);
            port.store(port.word(SECOND_COUNT), rank);
            let folded = folded_count(port, frame.position, found.page);
            port.store(port.word(FIRST_COUNT), folded);
            RawStatus(0)
        }
        Err(raw) => raw,
    };
    complete(port, callback, raw);
    0
}
