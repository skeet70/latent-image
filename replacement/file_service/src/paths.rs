//! The media task's path workers: the file delete, directory delete and read-only requests
//! the retained request services carry out, the filename and attribute updates of a numbered
//! directory's entry, and the image remove. Each reads its posted payload, submits the work to
//! the retained path, catalog and filesystem services, translates the raw result and completes
//! the request. Offsets are into the payloads the posters build.

use crate::completion::RawStatus;
use crate::requests::Callback;
use crate::workers::Offset;

const CALLBACK: Offset = Offset(0);
const PATH: Offset = Offset(4);
/// The filename update's byte: nonzero skips the catalog rebuild.
const KEEP_CATALOG: Offset = Offset(0x34);
const ATTRIBUTE_VALUE: Offset = Offset(0x34);
const ATTRIBUTE_SELECTOR: Offset = Offset(0x38);
/// The image remove uppercases at most this many path bytes.
const PATH_BYTES: u32 = 48;

const NO_CONTEXT: u32 = 5;
const CANCELLED: u32 = 11;
/// A request service status after which no catalog reset follows.
const UNRESET: u32 = 6;
const RAW_NO_CONTEXT: RawStatus = RawStatus(2);
const RAW_OK: RawStatus = RawStatus(0);
const RAW_INVALID: RawStatus = RawStatus(4);
const RAW_READ_ONLY: RawStatus = RawStatus(17);
/// An entry update result the filename worker treats as success.
const RAW_ENTRY_KEPT: RawStatus = RawStatus(22);
/// A failed entry update result after which the filename worker resets no catalog.
const RAW_ENTRY_UNRESET: RawStatus = RawStatus(7);
/// The wildcard service's answer for a path with `*` or `?`.
const WILDCARD: u32 = 1;

/// The context cancel bytes the posters clear: +0x1f6 for the file delete, filename update and
/// image remove, +0x1f7 for the read-only and attribute updates. A byte of 1 cancels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cancel {
    Filename,
    Metadata,
}

/// The catalog rebuild's mode word: 1 after a completed request, 0 after a failed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rebuild {
    Completed,
    Failed,
}

/// The two retained entry updates the attribute worker selects between.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryService {
    /// 0x714940.
    Update,
    /// 0x713ef4.
    UpdateOne,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectoryNumber(pub u32);

/// The context's word for a directory number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectoryRecord(pub u32);

/// The file number the retained name parser reads from a split name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileNumber(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtensionId(pub u32);

/// The first byte of a device's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceState(pub u8);

impl DeviceState {
    fn refusal(self) -> Option<RawStatus> {
        if self.0 & 1 != 0 {
            Some(RawStatus(1))
        } else if self.0 & 0x1a != 0 {
            Some(RawStatus(2))
        } else if self.0 & 4 != 0 {
            Some(RawStatus(18))
        } else {
            None
        }
    }
}

/// The directory, name, word and flag the retained path splitter fills, sized as the stock
/// worker's frame.
#[repr(C)]
pub struct Split<const D: usize, const N: usize> {
    pub directory: [u8; D],
    pub name: [u8; N],
    pub word: u32,
    pub flag: u32,
}

impl<const D: usize, const N: usize> Default for Split<D, N> {
    fn default() -> Self {
        Split { directory: [0; D], name: [0; N], word: 0, flag: 0 }
    }
}

/// The record the retained catalog search fills for the image remove.
#[repr(C)]
pub struct Entry(pub [u8; 188]);

impl Default for Entry {
    fn default() -> Self {
        Entry([0; 188])
    }
}

impl Entry {
    fn half(&self, at: usize) -> u32 {
        u32::from(u16::from_le_bytes([self.0[at], self.0[at + 1]]))
    }

    fn directory(&self) -> DirectoryNumber {
        DirectoryNumber(self.half(0x6c))
    }

    fn file(&self) -> FileNumber {
        FileNumber(self.half(0x6e))
    }

    fn removable(&self) -> Result<(), RawStatus> {
        if self.0[0x7c] & 0x80 != 0 || self.0[0x7d] & 0xf == 0 {
            Err(RAW_INVALID)
        } else if self.0[0x7e] & 1 != 0 {
            Err(RAW_READ_ONLY)
        } else {
            Ok(())
        }
    }
}

/// The image remove's working storage, sized as the stock worker's frame.
pub struct RemoveFrame {
    pub split: Split<48, 48>,
    pub entry: Entry,
    pub device: [u8; 48],
}

impl Default for RemoveFrame {
    fn default() -> Self {
        RemoveFrame { split: Split::default(), entry: Entry::default(), device: [0; 48] }
    }
}

pub trait Port {
    fn word(&self, at: Offset) -> u32;
    fn set_word(&mut self, at: Offset, value: u32);
    fn byte(&self, at: Offset) -> u8;
    fn set_byte(&mut self, at: Offset, value: u8);
    /// The guest address of the payload bytes at `at`.
    fn address(&self, at: Offset) -> u32;

    /// Whether a media-state context is active.
    fn context(&self) -> bool;
    fn cancelled(&self, cancel: Cancel) -> bool;
    fn directory_record(&self, number: DirectoryNumber) -> DirectoryRecord;
    /// Refreshes the media state of the context's device.
    fn refresh_media(&mut self);
    /// Translates a raw filesystem result, reporting it when the translation reports.
    fn translate(&mut self, raw: RawStatus) -> u32;
    fn complete_status(&mut self, callback: Callback, status: u32);

    /// 0x71df30 with the request; returns a client status.
    fn delete_file(&mut self) -> u32;
    /// 0x71e08c with the request and its worker table row; returns a client status.
    fn delete_directory(&mut self) -> u32;
    /// 0x71e288 with the request; returns a client status.
    fn set_read_only(&mut self) -> u32;
    fn rebuild_catalog(&mut self, rebuild: Rebuild) -> RawStatus;
    fn reload_catalog(&mut self) -> RawStatus;

    fn split_path<const D: usize, const N: usize>(&mut self, path: u32, split: &mut Split<D, N>) -> RawStatus;
    fn wildcard(&mut self, path: u32) -> u32;
    fn directory_number<const D: usize, const N: usize>(&mut self, split: &Split<D, N>) -> DirectoryNumber;
    fn update_entry<const D: usize, const N: usize>(
        &mut self,
        service: EntryService,
        record: DirectoryRecord,
        split: &Split<D, N>,
        mode: u32,
        value: u32,
        wildcard: u32,
    ) -> RawStatus;

    fn device_state(&mut self, device: u32, state: &mut [u8; 48]) -> DeviceState;
    fn file_number(&mut self, name: &[u8; 48]) -> FileNumber;
    fn extension_id(&mut self, extension: u32) -> ExtensionId;
    fn extension_kind(&mut self, id: ExtensionId) -> u32;
    fn find_entry(&mut self, entry: &mut Entry) -> RawStatus;
    fn delete_path(&mut self, path: u32) -> RawStatus;
}

fn take_callback<P: Port>(port: &mut P) -> Callback {
    let callback = Callback(port.word(CALLBACK));
    port.set_word(CALLBACK, 0);
    callback
}

fn cancel<P: Port>(port: &mut P, callback: Callback) -> u32 {
    if cfg!(file_service_control_paths_cancel_restores_callback) {
        port.set_word(CALLBACK, callback.0);
    }
    port.complete_status(callback, CANCELLED);
    0
}

fn finish<P: Port>(port: &mut P, callback: Callback, raw: RawStatus) -> u32 {
    port.set_word(CALLBACK, callback.0);
    let status = port.translate(raw);
    port.complete_status(callback, status);
    status
}

fn after_request<P: Port>(port: &mut P, status: u32) -> u32 {
    if status == 0 {
        let raw = port.rebuild_catalog(Rebuild::Completed);
        return port.translate(raw);
    }
    if status != UNRESET {
        port.rebuild_catalog(Rebuild::Failed);
    }
    status
}

fn cancellable<P: Port>(port: &mut P, cancel_byte: Cancel, request: fn(&mut P) -> u32) -> u32 {
    let callback = take_callback(port);
    let status = if !port.context() {
        NO_CONTEXT
    } else if port.cancelled(cancel_byte) {
        return cancel(port, callback);
    } else {
        let status = request(port);
        after_request(port, status)
    };
    if !cfg!(file_service_control_paths_request_callback_unrestored) {
        port.set_word(CALLBACK, callback.0);
    }
    port.complete_status(callback, status);
    0
}

/// The file delete worker at 0x7118fc (code 0x2c0a).
pub fn file_delete<P: Port>(port: &mut P) -> u32 {
    cancellable(port, Cancel::Filename, P::delete_file)
}

/// The read-only worker at 0x711ea4 (code 0x2c0c).
pub fn read_only<P: Port>(port: &mut P) -> u32 {
    cancellable(port, Cancel::Metadata, P::set_read_only)
}

/// The directory delete worker at 0x711b50 (code 0x2c0b).
pub fn directory_delete<P: Port>(port: &mut P) -> u32 {
    let callback = take_callback(port);
    let status = if port.context() {
        let status = port.delete_directory();
        if status == 0 {
            let raw = port.rebuild_catalog(Rebuild::Completed);
            port.translate(raw)
        } else {
            status
        }
    } else {
        NO_CONTEXT
    };
    port.set_word(CALLBACK, callback.0);
    port.complete_status(callback, status);
    if cfg!(file_service_control_paths_directory_delete_returns_zero) {
        0
    } else {
        status
    }
}

fn numbered_record<P: Port, const D: usize, const N: usize>(port: &mut P, split: &Split<D, N>) -> Option<DirectoryRecord> {
    let number = port.directory_number(split);
    if number.0 == 0 {
        return None;
    }
    let record = port.directory_record(number);
    (record.0 != 0 && record.0 != u32::MAX && number.0 >= 100).then_some(record)
}

/// The filename worker at 0x7119b0 (code 0x2c03).
pub fn filename_set<P: Port>(port: &mut P) -> u32 {
    let callback = take_callback(port);
    let keep_catalog = port.byte(KEEP_CATALOG) != 0;
    let raw = if !port.context() {
        RAW_NO_CONTEXT
    } else if port.cancelled(Cancel::Filename) {
        return cancel(port, callback);
    } else {
        update_filename(port, keep_catalog)
    };
    let status = finish(port, callback, raw);
    if cfg!(file_service_control_paths_filename_returns_status) {
        status
    } else {
        0
    }
}

fn update_filename<P: Port>(port: &mut P, keep_catalog: bool) -> RawStatus {
    let path = port.address(PATH);
    let mut split = Split::<32, 16>::default();
    let raw = port.split_path(path, &mut split);
    let wildcard = port.wildcard(path);
    if raw != RAW_OK {
        return raw;
    }
    let raw = match numbered_record(port, &split) {
        Some(record) => {
            let raw = port.update_entry(EntryService::Update, record, &split, 0, 0, wildcard);
            port.refresh_media();
            if raw != RAW_OK && raw != RAW_ENTRY_KEPT {
                if raw != RAW_ENTRY_UNRESET {
                    port.rebuild_catalog(Rebuild::Failed);
                }
                return raw;
            }
            raw
        }
        None => {
            port.refresh_media();
            RAW_OK
        }
    };
    if keep_catalog {
        raw
    } else {
        port.rebuild_catalog(Rebuild::Completed)
    }
}

/// The attribute worker at 0x711f58 (code 0x2c04).
pub fn attribute_set<P: Port>(port: &mut P) -> u32 {
    let callback = take_callback(port);
    let selector = port.byte(ATTRIBUTE_SELECTOR);
    let raw = if !port.context() {
        RAW_NO_CONTEXT
    } else if port.cancelled(Cancel::Metadata) {
        return cancel(port, callback);
    } else {
        update_attribute(port, selector)
    };
    finish(port, callback, raw);
    0
}

fn update_attribute<P: Port>(port: &mut P, selector: u8) -> RawStatus {
    let path = port.address(PATH);
    let mut split = Split::<32, 16>::default();
    let raw = port.split_path(path, &mut split);
    let wildcard = port.wildcard(path);
    if raw != RAW_OK {
        return raw;
    }
    match (numbered_record(port, &split), selector) {
        (None, 0) => port.reload_catalog(),
        (None, _) => RAW_OK,
        (Some(record), 0) => {
            let raw = port.update_entry(EntryService::Update, record, &split, 1, port.word(ATTRIBUTE_VALUE), wildcard);
            if raw == RAW_OK {
                port.reload_catalog()
            } else {
                if !cfg!(file_service_control_paths_attribute_failure_unreloaded) {
                    port.reload_catalog();
                }
                raw
            }
        }
        (Some(record), 1) => port.update_entry(EntryService::UpdateOne, record, &split, 1, port.word(ATTRIBUTE_VALUE), wildcard),
        (Some(_), _) => RAW_OK,
    }
}

/// The image remove worker at 0x711bc8 (code 0x2c25). The callback word stays 0.
pub fn image_remove<P: Port>(port: &mut P) -> u32 {
    let callback = take_callback(port);
    let raw = if port.context() {
        match remove_image(port) {
            Ok(raw) | Err(raw) => raw,
        }
    } else {
        RAW_NO_CONTEXT
    };
    if cfg!(file_service_control_paths_remove_restores_callback) {
        port.set_word(CALLBACK, callback.0);
    }
    let status = port.translate(raw);
    port.complete_status(callback, status);
    0
}

/// Returns the count of path bytes processed, the terminator included.
fn uppercase_path<P: Port>(port: &mut P) -> u32 {
    let mut index = 0;
    loop {
        let at = Offset(PATH.0 + index);
        let byte = port.byte(at);
        if byte.is_ascii_lowercase() {
            port.set_byte(at, byte - 32);
        }
        index += 1;
        if byte == 0 || index >= PATH_BYTES {
            return index;
        }
    }
}

fn check(refused: bool) -> Result<(), RawStatus> {
    if refused {
        Err(RAW_INVALID)
    } else {
        Ok(())
    }
}

fn succeeded(raw: RawStatus) -> Result<(), RawStatus> {
    if raw == RAW_OK {
        Ok(())
    } else {
        Err(raw)
    }
}

/// A drive-qualified path naming the catalog's current entry, with its extension's dot among
/// the last five bytes processed and a known extension kind, is deleted and the catalog rebuilt.
fn remove_image<P: Port>(port: &mut P) -> Result<RawStatus, RawStatus> {
    let processed = uppercase_path(port);
    let drive = port.byte(PATH);
    check(drive < b'A' || port.byte(Offset(PATH.0 + 1)) != b':' || drive > b'Z')?;
    let mut frame = RemoveFrame::default();
    if let Some(raw) = port.device_state(u32::from(drive - b'A'), &mut frame.device).refusal() {
        return Err(raw);
    }
    let path = port.address(PATH);
    succeeded(port.split_path(path, &mut frame.split))?;
    let directory = port.directory_number(&frame.split);
    let file = port.file_number(&frame.split.name);
    check(file.0 == 0 || directory.0 == 0)?;
    check(port.wildcard(path) == WILDCARD)?;
    succeeded(port.find_entry(&mut frame.entry))?;
    check(frame.entry.directory() != directory || frame.entry.file() != file)?;
    check(processed <= 5)?;
    let Some(dot) = (processed - 5..processed).find(|&index| port.byte(Offset(PATH.0 + index)) == b'.') else {
        port.extension_id(path);
        return Err(RAW_INVALID);
    };
    let id = port.extension_id(path + dot + 1);
    check(id.0 == 0)?;
    check(port.extension_kind(id) == 0)?;
    frame.entry.removable()?;
    succeeded(port.delete_path(path))?;
    Ok(port.rebuild_catalog(Rebuild::Completed))
}
