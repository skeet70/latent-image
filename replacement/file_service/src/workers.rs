//! The media task's handle workers: open, write, seek, close, close with an entry update and
//! release. Each reads its posted payload, moves the tag map, hands the file operation to the
//! retained filesystem, translates the raw result and completes the request. Offsets are into
//! the payloads the posters build.

use crate::completion::RawStatus;
use crate::handles::FileIndex;
use crate::requests::Callback;

/// A byte offset into a posted request payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset(pub u32);

/// The 48 payload bytes from the path that an open hands the filesystem.
pub type Name = [u8; 48];

/// The mode byte the filesystem open takes, selected by the payload's mode word 0 to 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenMode(pub u8);

impl OpenMode {
    pub fn from_word(word: u32) -> Option<OpenMode> {
        match word {
            0 => Some(OpenMode(b'r')),
            1 => Some(OpenMode(b'w')),
            2 => Some(OpenMode(b'a')),
            3 => Some(OpenMode(b's')),
            _ => None,
        }
    }
}

/// The working storage an open lends the path services, in stock's frame order.
#[repr(C)]
#[derive(Default)]
pub struct PathFrame {
    pub directory: [u8; 32],
    pub name: [u8; 12],
    pub parsed: u32,
    pub split_word: u32,
    pub split_flag: u32,
}

/// A blocking caller's completion words: the flag, the status pointer and, for open and
/// write, the value pointer.
#[derive(Clone, Copy)]
struct Blocking {
    flag: Offset,
    status: Offset,
    value: Option<Offset>,
}

const CALLBACK: Offset = Offset(0);
const HANDLE: Offset = Offset(4);

mod open {
    use super::{Blocking, Offset};
    pub const PATH: Offset = Offset(4);
    pub const DELIVERED: Offset = Offset(0x34);
    pub const MODE: Offset = Offset(0x38);
    pub const TAG: Offset = Offset(0x3c);
    pub const WORD: Offset = Offset(0x4c);
    pub const BLOCKING: Blocking = Blocking { flag: Offset(0x40), status: Offset(0x44), value: Some(Offset(0x48)) };
}

mod write {
    use super::{Blocking, Offset};
    pub const LENGTH: Offset = Offset(8);
    pub const SOURCE: Offset = Offset(0xc);
    pub const BLOCKING: Blocking = Blocking { flag: Offset(0x10), status: Offset(0x14), value: Some(Offset(0x18)) };
}

mod seek {
    use super::{Blocking, Offset};
    pub const LOW: Offset = Offset(8);
    pub const HIGH: Offset = Offset(0xc);
    pub const WHENCE: Offset = Offset(0x10);
    pub const BLOCKING: Blocking = Blocking { flag: Offset(0x14), status: Offset(0x18), value: None };
}

mod close {
    use super::{Blocking, Offset};
    pub const WORD: Offset = Offset(8);
    pub const NAME: Offset = Offset(0xc);
    pub const BLOCKING: Blocking = Blocking { flag: Offset(0x20), status: Offset(0x24), value: None };
}

mod entry {
    use super::Offset;
    pub const ATTRIBUTE: Offset = Offset(8);
    pub const FIRST: Offset = Offset(9);
    pub const SECOND: Offset = Offset(0x1d);
}

mod release {
    use super::{Blocking, Offset};
    pub const BLOCKING: Blocking = Blocking { flag: Offset(8), status: Offset(0xc), value: None };
}

/// The worker's status without an active media-state context, delivered untranslated.
const NO_CONTEXT: u32 = 5;
const RAW_NO_CONTEXT: RawStatus = RawStatus(2);
const RAW_BAD_MODE: RawStatus = RawStatus(19);
const RAW_NOT_OPEN: RawStatus = RawStatus(36);
const NO_SLOT: u32 = 0xff;
/// Mode words whose opens register the file and whose closes run the post-close actions.
const WRITTEN_MODES: [u32; 2] = [1, 3];
const KIND_FLAGGED: u32 = 1;
const KIND_RESCAN: u32 = 0x100;

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    fn set_word(&mut self, at: Offset, value: u32);
    fn byte(&self, at: Offset) -> u8;
    /// The guest address of the payload bytes at `at`.
    fn address(&self, at: Offset) -> u32;
    fn name(&self, at: Offset) -> Name;
    /// Stores to a caller's completion word.
    fn store(&mut self, address: u32, value: u32);

    /// Whether a media-state context is active.
    fn context(&self) -> bool;
    /// The open mode word recorded for a file index.
    fn file_mode(&self, index: FileIndex) -> u32;
    fn set_file_mode(&mut self, index: FileIndex, mode: u32);
    /// The kind word recorded for a file index.
    fn file_kind(&self, index: FileIndex) -> u32;
    /// Sets the context word a closed file of kind 1 marks.
    fn mark_flagged_close(&mut self);
    fn directory_known(&self, directory: u16) -> bool;

    fn release_tag(&mut self, tag: u32);
    fn lookup(&mut self, tag: u32) -> FileIndex;
    fn unbind(&mut self, tag: u32);
    fn bind(&mut self, tag: u32) -> FileIndex;
    /// Translates a raw filesystem result, reporting it when the translation reports.
    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete(&mut self, callback: Callback, status: u32, value: u32);
    fn complete_status(&mut self, callback: Callback, status: u32);
    /// Signals the blocking caller's completion.
    fn done(&mut self);

    fn parse_path(&mut self, frame: &mut PathFrame, path: u32) -> u32;
    fn directory_slot(&mut self, parsed: u32) -> u32;
    fn split_path(&mut self, path: u32, frame: &mut PathFrame);
    fn directory_number(&mut self, frame: &PathFrame) -> u32;
    fn make_directory(&mut self, directory: u16, slot: u32);
    fn fs_open(&mut self, name: &Name, mode: OpenMode, index: FileIndex, word: u32) -> RawStatus;
    fn register_written(&mut self, index: FileIndex, name: &Name);
    /// Writes `length` bytes from `source`; returns the count written.
    fn fs_write(&mut self, index: FileIndex, source: u32, length: u32) -> (u32, RawStatus);
    /// `pad` is the word stock leaves in r1 before the 64-bit offset.
    fn fs_seek(&mut self, index: FileIndex, pad: u32, low: u32, high: u32, whence: u32) -> RawStatus;
    fn fs_close(&mut self, index: FileIndex) -> RawStatus;
    fn fs_close_named(&mut self, index: FileIndex, word: u32, name: u32) -> RawStatus;
    fn fs_close_entry(&mut self, index: FileIndex, attribute: u8, first: u32, second: u32) -> RawStatus;
    fn fs_release(&mut self, index: FileIndex) -> RawStatus;
    fn cache_release(&mut self, index: FileIndex);
    /// Refreshes the media state of the context's device.
    fn refresh_media(&mut self);
    fn after_written_close(&mut self, index: FileIndex);
    fn rescan(&mut self) -> u32;
}

fn deliver<P: Port>(port: &mut P, blocking: Blocking, status: u32, value: u32) {
    if port.word(blocking.flag) == 0 {
        return;
    }
    let at = port.word(blocking.status);
    if at != 0 {
        port.store(at, status);
    }
    if let Some(pointer) = blocking.value {
        let at = port.word(pointer);
        if at != 0 {
            port.store(at, value);
        }
    }
    port.done();
}

fn handle_index<P: Port>(port: &mut P) -> FileIndex {
    let index = port.lookup(port.word(HANDLE));
    port.set_word(HANDLE, index.0);
    index
}

/// Releases the handle's tag and map pair; returns its file index, stored over the handle.
fn retire_handle<P: Port>(port: &mut P) -> FileIndex {
    let tag = port.word(HANDLE);
    if !cfg!(file_service_control_close_keeps_tag) {
        port.release_tag(tag);
    }
    let index = port.lookup(tag);
    port.set_word(HANDLE, index.0);
    port.unbind(tag);
    index
}

/// The open worker at 0x7115a4. Mode words 1 to 3 first create the path's numbered
/// directory when it is missing.
pub fn open<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let status = if port.context() {
        port.set_word(CALLBACK, 0);
        if port.word(open::MODE).wrapping_sub(1) <= 2 {
            prepare_directory(port);
        }
        open_file(port)
    } else {
        NO_CONTEXT
    };
    let value = port.word(open::DELIVERED);
    port.complete(callback, status, value);
    let value = port.word(open::DELIVERED);
    deliver(port, open::BLOCKING, status, value);
    status
}

fn prepare_directory<P: Port>(port: &mut P) {
    let path = port.address(open::PATH);
    let mut frame = PathFrame::default();
    if port.parse_path(&mut frame, path) != 0 {
        return;
    }
    let slot = port.directory_slot(frame.parsed);
    if slot == NO_SLOT {
        return;
    }
    port.split_path(path, &mut frame);
    let directory = port.directory_number(&frame) as u16;
    if directory.wrapping_sub(100) >= 900 || port.directory_known(directory) {
        return;
    }
    port.make_directory(directory, slot);
}

/// 0x71f2a8: binds the tag to a file index and opens the file; a failure releases the
/// index, the pair and the tag and delivers no tag.
fn open_file<P: Port>(port: &mut P) -> u32 {
    let tag = port.word(open::TAG);
    port.set_word(open::DELIVERED, tag);
    let index = port.bind(tag);
    port.set_word(open::TAG, index.0);
    let callback = Callback(port.word(CALLBACK));
    let name = port.name(open::PATH);
    let raw = if port.context() {
        let raw = match OpenMode::from_word(port.word(open::MODE)) {
            Some(mode) => port.fs_open(&name, mode, FileIndex(port.word(open::TAG)), port.word(open::WORD)),
            None => RAW_BAD_MODE,
        };
        if raw == RawStatus(0) {
            port.set_file_mode(FileIndex(port.word(open::TAG)), port.word(open::MODE));
            if WRITTEN_MODES.contains(&port.word(open::MODE)) {
                port.register_written(FileIndex(port.word(open::TAG)), &name);
            }
        }
        raw
    } else {
        RAW_NO_CONTEXT
    };
    if raw != RawStatus(0) {
        port.cache_release(FileIndex(port.word(open::TAG)));
        let delivered = port.word(open::DELIVERED);
        if !cfg!(file_service_control_open_failure_keeps_pair) {
            port.unbind(delivered);
        }
        port.release_tag(port.word(open::DELIVERED));
        port.set_word(open::DELIVERED, 0);
    }
    let status = port.translate(raw);
    let delivered = port.word(open::DELIVERED);
    port.complete(callback, status, delivered);
    status
}

/// The write worker at 0x7116e4: delivers the status and the count written.
pub fn write<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    let (status, count) = if port.context() {
        port.set_word(CALLBACK, 0);
        let status = write_file(port);
        (status, port.word(write::LENGTH))
    } else {
        (NO_CONTEXT, 0)
    };
    port.complete(callback, status, count);
    deliver(port, write::BLOCKING, status, count);
    status
}

/// 0x71ed34: the count written replaces the payload's length.
fn write_file<P: Port>(port: &mut P) -> u32 {
    let index = if cfg!(file_service_control_write_index_unstored) {
        port.lookup(port.word(HANDLE))
    } else {
        handle_index(port)
    };
    let callback = Callback(port.word(CALLBACK));
    let (count, raw) = if port.context() {
        port.fs_write(index, port.word(write::SOURCE), port.word(write::LENGTH))
    } else {
        (0, RAW_NO_CONTEXT)
    };
    let status = port.translate(raw);
    port.set_word(write::LENGTH, count);
    port.complete(callback, status, count);
    status
}

/// The seek worker at 0x71eca4.
pub fn seek<P: Port>(port: &mut P) -> u32 {
    let index = handle_index(port);
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() {
        let whence = port.word(seek::WHENCE);
        port.fs_seek(index, whence, port.word(seek::LOW), port.word(seek::HIGH), whence)
    } else {
        RAW_NO_CONTEXT
    };
    let status = port.translate(raw);
    port.complete_status(callback, status);
    deliver(port, seek::BLOCKING, status, 0);
    status
}

/// The close worker at 0x711768. Its callback and blocking caller receive the status;
/// the worker returns 0.
pub fn close<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    port.set_word(CALLBACK, 0);
    let status = if port.context() {
        let status = close_file(port);
        if status == 0 {
            after_close(port)
        } else {
            status
        }
    } else {
        NO_CONTEXT
    };
    port.complete_status(callback, status);
    deliver(port, close::BLOCKING, status, 0);
    0
}

/// The close worker with an entry update at 0x711840. It has no blocking caller.
pub fn close_entry<P: Port>(port: &mut P) -> u32 {
    let callback = Callback(port.word(CALLBACK));
    port.set_word(CALLBACK, 0);
    let status = if port.context() {
        let status = close_entry_file(port);
        if status == 0 {
            after_close(port)
        } else {
            status
        }
    } else {
        NO_CONTEXT
    };
    port.complete_status(callback, status);
    0
}

/// Files opened for writing in place get the post-close actions; a kind 0x100 file's status
/// is the rescan's untranslated result.
fn after_close<P: Port>(port: &mut P) -> u32 {
    let index = FileIndex(port.word(HANDLE));
    if !WRITTEN_MODES.contains(&port.file_mode(index)) {
        return 0;
    }
    port.after_written_close(index);
    if port.file_kind(FileIndex(port.word(HANDLE))) == KIND_FLAGGED && !cfg!(file_service_control_flagged_close_unmarked) {
        port.mark_flagged_close();
    }
    if port.file_kind(FileIndex(port.word(HANDLE))) == KIND_RESCAN {
        return port.rescan();
    }
    0
}

/// 0x71f1d4: an open file with a nonzero word or name closes through the named close.
fn close_file<P: Port>(port: &mut P) -> u32 {
    let index = retire_handle(port);
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() {
        let raw = if port.file_mode(index) != 0 && (port.word(close::WORD) != 0 || port.byte(close::NAME) != 0) {
            port.fs_close_named(index, port.word(close::WORD), port.address(close::NAME))
        } else {
            port.fs_close(index)
        };
        port.cache_release(FileIndex(port.word(HANDLE)));
        raw
    } else {
        RAW_NO_CONTEXT
    };
    let status = port.translate(raw);
    port.refresh_media();
    port.complete_status(callback, status);
    status
}

/// 0x71f078: a file index with no recorded mode is not open.
fn close_entry_file<P: Port>(port: &mut P) -> u32 {
    let index = retire_handle(port);
    let callback = Callback(port.word(CALLBACK));
    let raw = if !port.context() {
        RAW_NO_CONTEXT
    } else if port.file_mode(index) == 0 {
        RAW_NOT_OPEN
    } else {
        let raw = port.fs_close_entry(index, port.byte(entry::ATTRIBUTE), port.address(entry::FIRST), port.address(entry::SECOND));
        port.cache_release(FileIndex(port.word(HANDLE)));
        raw
    };
    let status = port.translate(raw);
    port.refresh_media();
    port.complete_status(callback, status);
    status
}

/// The release worker at 0x71f134.
pub fn release<P: Port>(port: &mut P) -> u32 {
    let index = retire_handle(port);
    let callback = Callback(port.word(CALLBACK));
    let raw = if port.context() {
        let raw = port.fs_release(index);
        port.cache_release(FileIndex(port.word(HANDLE)));
        raw
    } else {
        RAW_NO_CONTEXT
    };
    let status = port.translate(raw);
    port.complete_status(callback, status);
    deliver(port, release::BLOCKING, status, 0);
    status
}
