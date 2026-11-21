//! The select-image worker at 0x719098 (code 0x2c1f, poster 0x711078). It parses the posted
//! image path, searches the catalog pages whose directory path matches it, makes the file the
//! retained finder locates the media state's current file and refreshes the numbering
//! snapshot; any failure after the path parses reloads the current page's window instead.

use crate::completion::RawStatus;
use crate::requests::Callback;

/// A byte offset into the media-state context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContextOffset(pub u32);

/// A catalog page; pages 100 to 999 are numbered directories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Page(pub u32);

/// A file position in the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position(pub u32);

/// The extension kind 0x713264 finds in the context's extension table, 0 for none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionKind(pub u32);

/// The query 0x7189b4 and 0x717a24 receive, in stock's frame order.
#[repr(C)]
pub struct Query {
    pub search: u32,
    pub cluster: u32,
    pub selected: u32,
    pub low: u32,
    pub high: u32,
    /// The file tag 0x7189b4 looks for.
    pub tag: u32,
    pub page: u32,
    pub pattern: *const u8,
}

/// The worker's working storage, laid out as stock's frame from r11 - 0xb4.
#[repr(C)]
pub struct Frame {
    pub query: Query,
    pub cluster: u32,
    pub position: u32,
    pub secondary: u32,
    pub selected: u32,
    pub marker: u32,
    pub pattern: [u8; 12],
    pub directory: [u8; 16],
    pub found: [u8; 32],
    pub prefix: [u8; 32],
}

impl Frame {
    fn new() -> Frame {
        Frame {
            query: Query { search: 0, cluster: 0, selected: 0, low: 0, high: 0, tag: 0, page: 0, pattern: core::ptr::null() },
            cluster: 0,
            position: 0,
            secondary: 0,
            selected: 0,
            marker: 0,
            pattern: [0; 12],
            directory: [0; 16],
            found: [0; 32],
            prefix: [0; 32],
        }
    }
}

const OK: RawStatus = RawStatus(0);
const RAW_NO_CONTEXT: RawStatus = RawStatus(2);
const RAW_UNSUPPORTED: RawStatus = RawStatus(4);
const RAW_NOT_FOUND: RawStatus = RawStatus(7);

const OPEN_COUNT: ContextOffset = ContextOffset(0x2148);
const CURRENT_POSITION: ContextOffset = ContextOffset(0x214c);
const PAGE_FILES: ContextOffset = ContextOffset(0x2150);
const COUNTED_FILES: ContextOffset = ContextOffset(0x2154);
const MARKED_BASE: ContextOffset = ContextOffset(0x2158);
const MARKED_FILES: ContextOffset = ContextOffset(0x215c);
const FILE_TOTAL: ContextOffset = ContextOffset(0x2160);
const CURRENT_TAG: ContextOffset = ContextOffset(0x2164);
const PAGE: ContextOffset = ContextOffset(0x2170);
const POSITION: ContextOffset = ContextOffset(0x2174);
const MARKED_RANGE: [ContextOffset; 2] = [ContextOffset(0x2178), ContextOffset(0x217c)];
const DIRECTORY_RANGE: [ContextOffset; 2] = [ContextOffset(0x218c), ContextOffset(0x2190)];
const LOADED_COUNT: ContextOffset = ContextOffset(0x21a4);
const LOADED_MARKED: ContextOffset = ContextOffset(0x21b0);
const LOADED_MARKED_EXTRA: ContextOffset = ContextOffset(0x21b4);
const SELECTION_MASK: ContextOffset = ContextOffset(0x2210);
const SELECTED_TOTAL: ContextOffset = ContextOffset(0x2214);
/// Bytes per page: the page's counter index, 0xff for none.
const PAGE_COUNTERS: u32 = 0x2218;
/// Words per directory number: 0 or -1 for an unknown directory.
const DIRECTORIES: u32 = 0x2604;
/// Halfwords per page from page 0.
const PAGE_SIZES: u32 = 0x34dc;
/// Words per position modulo 5000; the low 24 bits are the file's tag.
const FILE_TAGS: u32 = 0xb2a4;
/// Halfwords per page from page 0.
const PAGE_FILE_COUNTS: u32 = 0x1270e;
/// Halfwords per group number.
const GROUP_SIZES: u32 = 0x156b6;
/// Bytes per position: bit 0 and the zero byte count a file; the rest is the group number.
const FILE_FLAGS: u32 = 0x15846;
const SNAPSHOT_FILES: ContextOffset = ContextOffset(0x16bce);
const GROUP_FILES: ContextOffset = ContextOffset(0x16c60);

const WINDOW: u32 = 5000;
const FIRST_NUMBERED: u32 = 100;
const UNKNOWN_DIRECTORY: u32 = 0xffff_ffff;

pub trait Port {
    fn callback(&self) -> Callback;
    /// The guest address of the payload's path.
    fn path(&self) -> u32;
    fn context(&self) -> bool;
    fn word(&self, at: ContextOffset) -> u32;
    fn set_word(&mut self, at: ContextOffset, value: u32);
    fn half(&self, at: ContextOffset) -> u16;
    fn set_half(&mut self, at: ContextOffset, value: u16);
    fn byte(&self, at: ContextOffset) -> u8;

    /// 0x70e664 with 0 over the 32 bytes.
    fn clear(&mut self, buffer: &mut [u8; 32]);
    /// 0x71543c into the frame's directory, pattern, marker, prefix and secondary marker.
    fn split_path(&mut self, path: u32, frame: &mut Frame) -> RawStatus;
    /// 0x713264 on the pattern's extension bytes.
    fn extension_kind(&mut self, frame: &Frame) -> ExtensionKind;
    /// 0x713508.
    fn kind_supported(&mut self, kind: ExtensionKind) -> u32;
    /// 0x712dac on the frame's directory and marker.
    fn directory_number(&mut self, frame: &Frame) -> u32;
    /// 0x713d60 into the frame's cluster.
    fn page_cluster(&mut self, page: Page, frame: &mut Frame) -> RawStatus;
    /// 0x713db8 with flag 0 into the frame's found path.
    fn directory_path(&mut self, frame: &mut Frame) -> RawStatus;
    /// 0x70e884 on the found path and the prefix: whether they differ.
    fn paths_differ(&mut self, frame: &Frame) -> bool;
    /// 0x7189b4 on the frame's query into its position.
    fn find(&mut self, frame: &mut Frame) -> RawStatus;
    /// 0x7153a8.
    fn marked_before(&mut self, position: Position) -> u32;
    /// 0x715f80.
    fn page_counted(&mut self, page: Page) -> u32;
    /// 0x712b4c.
    fn refresh_snapshot(&mut self);
    /// 0x715918 into the frame's selected word.
    fn page_selected(&mut self, page: Page, frame: &mut Frame) -> RawStatus;
    /// 0x717a24 on the frame's query, with r1 and r2 0.
    fn load_window(&mut self, frame: &mut Frame);

    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete_status(&mut self, callback: Callback, status: u32);
}

/// The worker at 0x719098; it returns 0.
pub fn select<P: Port>(port: &mut P) -> u32 {
    let callback = port.callback();
    let raw = if port.context() { select_file(port) } else { RAW_NO_CONTEXT };
    let status = port.translate(raw);
    port.complete_status(callback, status);
    0
}

fn select_file<P: Port>(port: &mut P) -> RawStatus {
    let mut frame = Frame::new();
    port.clear(&mut frame.prefix);
    let path = port.path();
    let parsed = port.split_path(path, &mut frame);
    if parsed != OK {
        return parsed;
    }
    let raw = search(port, &mut frame);
    if raw != OK {
        reload(port, &mut frame);
    }
    raw
}

fn known<P: Port>(port: &P, number: u16) -> bool {
    let entry = port.word(ContextOffset(DIRECTORIES + 4 * u32::from(number)));
    entry != 0 && entry != UNKNOWN_DIRECTORY
}

/// Searches the pages a numbered known directory names, or else the marked or directory page
/// range, for the first page whose directory path equals the parsed prefix.
fn search<P: Port>(port: &mut P, frame: &mut Frame) -> RawStatus {
    let kind = port.extension_kind(frame);
    if kind.0 == 0 || port.kind_supported(kind) == 0 {
        return RAW_UNSUPPORTED;
    }
    frame.selected = 0;
    let number = port.directory_number(frame) as u16;
    let (first, last) = if u32::from(number) >= FIRST_NUMBERED && known(port, number) {
        frame.selected = 1;
        (u32::from(number), u32::from(number))
    } else {
        let range = if frame.marker != 0 {
            MARKED_RANGE
        } else {
            if number != 0 && known(port, number) {
                frame.selected = 1;
            }
            DIRECTORY_RANGE
        };
        let (first, last) = (port.word(range[0]), port.word(range[1]));
        if first > last {
            return RAW_NOT_FOUND;
        }
        (first, last)
    };
    let mut raw;
    let mut page = first;
    loop {
        raw = port.page_cluster(Page(page), frame);
        if raw == OK && frame.cluster != 0 {
            port.clear(&mut frame.found);
            if port.directory_path(frame) == OK && !port.paths_differ(frame) {
                frame.query = Query {
                    search: 1,
                    cluster: frame.cluster,
                    selected: frame.selected,
                    low: 0,
                    high: WINDOW - 1,
                    tag: u32::MAX,
                    page,
                    pattern: frame.pattern.as_ptr(),
                };
                let found = port.find(frame);
                if found == OK {
                    make_current(port, Page(page), Position(frame.position), number);
                }
                return found;
            }
        }
        page = page.wrapping_add(1);
        if page > last {
            break;
        }
    }
    if raw == OK { RAW_NOT_FOUND } else { raw }
}

fn make_current<P: Port>(port: &mut P, page: Page, position: Position, number: u16) {
    port.set_word(POSITION, position.0);
    port.set_word(PAGE, page.0);
    let count = port.word(LOADED_COUNT).wrapping_add(1);
    port.set_word(OPEN_COUNT, count);
    port.set_word(CURRENT_POSITION, position.0);
    let marked = port.word(LOADED_MARKED);
    port.set_word(MARKED_BASE, marked);
    let marked = port.marked_before(position).wrapping_add(port.word(LOADED_MARKED_EXTRA));
    port.set_word(MARKED_FILES, marked);
    let tag_mask = if cfg!(file_service_control_selection_tag_untruncated) { u32::MAX } else { 0x00ff_ffff };
    let tag = port.word(ContextOffset(FILE_TAGS + 4 * (position.0 % WINDOW))) & tag_mask;
    port.set_word(CURRENT_TAG, tag);

    let page = Page(port.word(PAGE));
    if port.page_counted(page) == 0 {
        let counter = port.byte(ContextOffset(PAGE_COUNTERS.wrapping_add(page.0)));
        if counter != 0xff {
            port.set_word(SELECTION_MASK, 1u32.checked_shl(u32::from(counter)).unwrap_or(0));
        }
    }

    let position = port.word(POSITION);
    port.set_word(FILE_TOTAL, position);
    port.set_word(SELECTED_TOTAL, position);
    let current = port.word(CURRENT_POSITION);
    let group = port.byte(ContextOffset(FILE_FLAGS.wrapping_add(current))) >> 1;
    let files = if group == 0 { 0 } else { u32::from(port.half(ContextOffset(GROUP_SIZES + 2 * u32::from(group)))) };
    port.set_word(GROUP_FILES, files);

    let last = port.word(POSITION);
    let mut counted: u16 = 0xffff;
    let mut index: u32 = 0;
    loop {
        let flags = port.byte(ContextOffset(FILE_FLAGS.wrapping_add(index)));
        if flags == 0 || (!cfg!(file_service_control_selection_odd_uncounted) && flags & 1 != 0) {
            counted = counted.wrapping_add(1);
        }
        index = index.wrapping_add(1);
        if index > last {
            break;
        }
    }
    let page = port.word(PAGE);
    let files = port.half(ContextOffset(PAGE_FILE_COUNTS.wrapping_add(page.wrapping_mul(2))));
    port.set_word(PAGE_FILES, u32::from(files));
    port.set_word(COUNTED_FILES, u32::from(counted));

    for directory in FIRST_NUMBERED..u32::from(number) {
        let size = u32::from(port.half(ContextOffset(PAGE_SIZES + 2 * directory)));
        let total = size.wrapping_add(port.word(FILE_TOTAL));
        port.set_word(FILE_TOTAL, total);
        counted = counted.wrapping_add(port.half(ContextOffset(PAGE_FILE_COUNTS + 2 * directory)));
        if port.page_counted(Page(directory)) == 1 {
            let size = u32::from(port.half(ContextOffset(PAGE_SIZES + 2 * directory)));
            let total = size.wrapping_add(port.word(SELECTED_TOTAL));
            if !cfg!(file_service_control_selection_selected_total_skipped) {
                port.set_word(SELECTED_TOTAL, total);
            }
        }
    }
    port.set_half(SNAPSHOT_FILES, counted);
    port.refresh_snapshot();
}

/// Reloads the 5000-position window holding the current position on the current page.
fn reload<P: Port>(port: &mut P, frame: &mut Frame) {
    if port.page_cluster(Page(port.word(PAGE)), frame) != OK || frame.cluster == 0 {
        return;
    }
    if port.page_selected(Page(port.word(PAGE)), frame) != OK {
        return;
    }
    frame.query.search = 0;
    frame.query.cluster = frame.cluster;
    frame.query.selected = frame.selected;
    let position = port.word(POSITION);
    let low = position - position % WINDOW;
    frame.query.low = low;
    frame.query.high = low.wrapping_add(WINDOW - 1);
    frame.query.page = port.word(PAGE);
    port.load_window(frame);
}
