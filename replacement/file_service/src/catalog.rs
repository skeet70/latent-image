//! The media task's catalog workers: group selection, catalog rebuild, catalog find, request
//! completion and file-index resolution. Each reads its posted payload, hands the catalog work
//! to the retained stock services and delivers the status. Offsets are into the payloads the
//! posters build.

use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::workers::Offset;

/// A directory number, 100 to 999.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Directory(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GroupMask(pub u32);

/// The context word at +0x21cc; the file-find worker rebuilds the catalog first when it is 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CatalogMark(pub u32);

/// The second argument of the catalog rebuild at 0x719de0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RebuildMode(pub u32);

/// The four payload words the file-find worker hands 0x71a93c as r0 to r3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindSelectors {
    pub mode: u32,
    pub commit: u32,
    pub step: u32,
    pub word: u32,
}

/// The words 0x717a24 receives in r0, in stock's frame order. `cluster` and `marked` are
/// written by 0x713d60 and 0x715918; `unset` is never written by stock.
#[repr(C)]
#[derive(Default)]
pub struct WindowQuery {
    pub start: u32,
    pub cluster: u32,
    pub marked: u32,
    pub first: u32,
    pub last: u32,
    pub cursor: u32,
    pub directory: u32,
    pub unset: u32,
}

/// The words 0x717a24 receives in r2 and its visitor 0x712a4c advances.
#[repr(C)]
#[derive(Default)]
pub struct Window {
    pub first_total: u32,
    pub second_total: u32,
    pub position: u32,
    pub name_sum: u32,
}

/// The working storage the file-index resolution lends the retained services, in stock's
/// frame order from fp-0x70.
#[repr(C)]
#[derive(Default)]
pub struct ResolveFrame {
    pub query: WindowQuery,
    pub window: Window,
    /// The file name 0x713db8 writes.
    pub name: [u8; 32],
    /// 0x7158a8's answer, passed to 0x713db8.
    pub reserved: u32,
}

const CALLBACK: Offset = Offset(0);

mod groups {
    use super::Offset;
    pub const MASK: Offset = Offset(4);
}

mod rebuild {
    use super::Offset;
    pub const MODE: Offset = Offset(8);
    pub const FULL: Offset = Offset(0xc);
}

mod find {
    use super::Offset;
    pub const RECORD: Offset = Offset(4);
    pub const MODE: Offset = Offset(8);
    pub const COMMIT: Offset = Offset(0xc);
    pub const STEP: Offset = Offset(0x10);
    pub const WORD: Offset = Offset(0x14);
    pub const NAME: Offset = Offset(0x18);
}

mod resolve {
    use super::Offset;
    pub const INDEX: Offset = Offset(4);
    pub const FIRST_AT: Offset = Offset(8);
    pub const SECOND_AT: Offset = Offset(0xc);
    pub const BLOCKING: Offset = Offset(0x10);
    pub const STATUS_AT: Offset = Offset(0x14);
}

/// The untranslated status of a refused group selection or file-index resolution.
const NOT_FOUND: u32 = 8;
const RAW_NO_CONTEXT: RawStatus = RawStatus(2);
const RAW_OK: RawStatus = RawStatus(0);
const PENDING: CatalogMark = CatalogMark(1);
const CLEAR: CatalogMark = CatalogMark(0);
const RECORD_BYTES: u32 = 188;
const FIRST_DIRECTORY: u16 = 100;
const DIRECTORY_END: u16 = 1000;
/// Files per window 0x717a24 scans.
const WINDOW_FILES: u32 = 5000;

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    fn byte(&self, at: Offset) -> u8;
    fn address(&self, at: Offset) -> u32;
    fn store(&mut self, address: u32, value: u32);

    fn context(&self) -> bool;
    /// The group count byte at context +0x2600, read without a context check.
    fn group_count(&self) -> u8;
    fn set_group_mask(&mut self, mask: GroupMask);
    fn catalog_mark(&self) -> CatalogMark;
    fn set_catalog_mark(&mut self, mark: CatalogMark);
    /// Clears the byte at context +0x1f5, also with no context.
    fn clear_selection(&mut self);
    /// The file count of a directory, a halfword at context +0x35a4.
    fn directory_count(&self, directory: Directory) -> u16;
    /// A directory's words at context +0x208 and +0x11a8.
    fn directory_totals(&self, directory: Directory) -> (u32, u32);

    /// Translates a raw result, reporting it when the translation reports.
    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete_status(&mut self, callback: Callback, status: u32);
    /// Signals the blocking caller's completion.
    fn done(&mut self);

    /// 0x719de0.
    fn rebuild_catalog(&mut self, full: bool, mode: RebuildMode) -> RawStatus;
    /// 0x70e664.
    fn fill(&mut self, address: u32, value: u8, length: u32);
    /// 0x71a93c; `name` is a payload address or 0.
    fn find_entry(&mut self, selectors: FindSelectors, name: u32, record: u32) -> RawStatus;
    /// 0x71985c.
    fn complete_request(&mut self) -> RawStatus;
    /// 0x715918 into `frame.query.marked`.
    fn directory_marked(&mut self, directory: Directory, frame: &mut ResolveFrame) -> RawStatus;
    /// 0x713d60 into `frame.query.cluster`.
    fn reserved_cluster(&mut self, directory: Directory, frame: &mut ResolveFrame) -> RawStatus;
    /// 0x7158a8 into `frame.reserved`.
    fn directory_reserved(&mut self, directory: Directory, frame: &mut ResolveFrame);
    /// 0x713db8 of `frame.query.cluster` into `frame.name`, with `frame.reserved` and 0.
    fn cluster_path(&mut self, frame: &mut ResolveFrame) -> RawStatus;
    /// 0x717a24 over `frame.query` with the visitor 0x712a4c and `frame.window`.
    fn scan_window(&mut self, frame: &mut ResolveFrame) -> RawStatus;
}

/// The group-selection worker at 0x71a834 (code 0x2c2b). A mask with no bit below the
/// context's group count is refused; the worker returns the delivered status.
pub fn select_groups<P: Port>(port: &mut P) -> u32 {
    let mask = GroupMask(port.word(groups::MASK));
    let callback = Callback(port.word(CALLBACK));
    let count = u32::from(port.group_count());
    let below = !u32::MAX.checked_shl(count).unwrap_or(0);
    let status = if mask.0 & below == 0 {
        NOT_FOUND
    } else {
        port.set_group_mask(if cfg!(file_service_control_catalog_group_mask_trimmed) { GroupMask(mask.0 & below) } else { mask });
        port.rebuild_catalog(false, RebuildMode(0));
        0
    };
    port.complete_status(callback, status);
    status
}

/// The catalog-rebuild worker at 0x71a8a0 (code 0x2c02). It records whether the mode was 1
/// and returns 0.
pub fn rebuild_catalog<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let full = port.byte(rebuild::FULL) == 1;
    let mode = RebuildMode(port.word(rebuild::MODE));
    let raw = if port.context() {
        let raw = port.rebuild_catalog(full, mode);
        port.set_catalog_mark(CatalogMark(u32::from(mode.0 == 1)));
        raw
    } else {
        RAW_NO_CONTEXT
    };
    let status = port.translate(raw);
    if !cfg!(file_service_control_catalog_rebuild_selection_kept) {
        port.clear_selection();
    }
    port.complete_status(callback, status);
    0
}

/// The catalog-find worker at 0x71b8c0 (code 0x2c0d). A pending mark or a mode 2 commit 1
/// request rebuilds the catalog first; the output record is cleared and filled by 0x71a93c.
pub fn find_entry<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let record = port.word(find::RECORD);
    let selectors = FindSelectors {
        mode: port.word(find::MODE),
        commit: port.word(find::COMMIT),
        step: port.word(find::STEP),
        word: port.word(find::WORD),
    };
    let raw = if port.context() { find_in_catalog(port, selectors, record) } else { RAW_NO_CONTEXT };
    let status = port.translate(raw);
    port.complete_status(callback, status);
    0
}

fn find_in_catalog<P: Port>(port: &mut P, selectors: FindSelectors, record: u32) -> RawStatus {
    if port.catalog_mark() == PENDING || (selectors.mode == 2 && selectors.commit == 1) {
        let raw = port.rebuild_catalog(false, RebuildMode(0));
        if !cfg!(file_service_control_catalog_find_mark_kept) {
            port.set_catalog_mark(CLEAR);
        }
        if raw != RAW_OK {
            return raw;
        }
    }
    port.fill(record, 0, RECORD_BYTES);
    let name = if port.byte(find::NAME) != 0 { port.address(find::NAME) } else { 0 };
    port.find_entry(selectors, name, record)
}

/// The request-completion worker at 0x71b9b0 (code 0x2c05). It returns 0.
pub fn complete_request<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let raw = port.complete_request();
    let status = port.translate(raw);
    port.complete_status(callback, status);
    if cfg!(file_service_control_catalog_complete_returns_status) { status } else { 0 }
}

/// The file-index resolution worker at 0x719614 (code 0x2c10): stores the found directory's
/// window totals plus the totals of every later directory to the payload's two pointers. It
/// returns 0.
pub fn resolve_index<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let mut frame = ResolveFrame::default();
    let status = match port.context().then(|| resolve_window(port, &mut frame)).flatten() {
        Some(directory) => {
            let (first, second) = later_totals(port, directory, &frame.window);
            let at = port.word(resolve::FIRST_AT);
            port.store(at, first);
            let at = port.word(resolve::SECOND_AT);
            port.store(at, second);
            0
        }
        None => NOT_FOUND,
    };
    port.complete_status(callback, status);
    if port.word(resolve::BLOCKING) != 0 {
        let at = port.word(resolve::STATUS_AT);
        if at != 0 {
            port.store(at, status);
        }
        port.done();
    }
    0
}

/// The directory holding the payload's file index, by the counts of directories 100 on.
fn locate<P: Port>(port: &P, index: u32) -> Option<(Directory, u32)> {
    let mut first = 0u32;
    for number in FIRST_DIRECTORY..DIRECTORY_END {
        let directory = Directory(number);
        let next = first + u32::from(port.directory_count(directory));
        if index < next {
            return Some((directory, first));
        }
        first = next;
    }
    None
}

/// A name shorter than 8 bytes refuses; the bytes after the eighth are summed into the window.
fn resolve_window<P: Port>(port: &mut P, frame: &mut ResolveFrame) -> Option<Directory> {
    let index = port.word(resolve::INDEX);
    let (directory, first) = locate(port, index)?;
    if port.directory_marked(directory, frame) != RAW_OK || port.reserved_cluster(directory, frame) != RAW_OK {
        return None;
    }
    port.directory_reserved(directory, frame);
    if port.cluster_path(frame) != RAW_OK || frame.name[..8].contains(&0) {
        return None;
    }
    let name_sum = frame.name[8..].iter().take_while(|byte| **byte != 0).fold(0u32, |sum, byte| sum + u32::from(*byte));
    let position = index - first;
    let window_first = position / WINDOW_FILES * WINDOW_FILES;
    frame.query.first = window_first;
    frame.query.last = window_first + (WINDOW_FILES - 1);
    frame.query.directory = u32::from(directory.0);
    frame.window.position = position;
    frame.window.name_sum = name_sum;
    if port.scan_window(frame) != RAW_OK {
        return None;
    }
    Some(directory)
}

fn later_totals<P: Port>(port: &P, directory: Directory, window: &Window) -> (u32, u32) {
    let start = if cfg!(file_service_control_catalog_resolve_totals_inclusive) { directory.0 } else { directory.0 + 1 };
    (start..DIRECTORY_END).fold((window.first_total, window.second_total), |(first, second), number| {
        let (more_first, more_second) = port.directory_totals(Directory(number));
        (first.wrapping_add(more_first), second.wrapping_add(more_second))
    })
}
