//! The overlay-46 save file machine: one message-registry
//! record that names and opens a file, writes a descriptor's scatter list into it, closes it and
//! calls the save completion, with the cancel and error paths of the stock state table.

pub const RECORD: u32 = 0x014d_dbfc;
pub const SAVED_PATH: u32 = 0x014d_dba8;
pub const PENDING: u32 = 0x014d_dbd8;
pub const OUTPUT_MODE: u32 = 0x014d_dbe4;

const STATE: u32 = 0x0c;
const MESSAGE: u32 = 0x18;
const OWNER: u32 = 0x1c;
const COMPLETION: u32 = 0x20;
const RESULT: u32 = 0x24;
const DESCRIPTOR: u32 = 0x28;

const MESSAGE_STATUS: u32 = 0x4;

const HANDLE: u32 = 0x0;
const KIND: u32 = 0x4;
const ENTRIES: u32 = 0x8;
const WRITE_INDEX: u32 = 0x88;
const NAME: u32 = 0x8c;
const PREFIX_SOURCE: u32 = 0x148;
const APPEND: u32 = 0x14c;

const NAME_BYTES: u32 = 48;
const ENTRY_COUNT: u32 = 16;
const LAST_CUT: u32 = if cfg!(sfm_control_name_cut_44) { 44 } else { 43 };
const LAST_APPEND_SLOT: u32 = if cfg!(sfm_control_append_full_at_14) { 13 } else { 14 };

const CODE_PREFIX_PROPERTY: u32 = 0x14f;
const VERSION_PREFIX_PROPERTY: u32 = 0x153;

/// The value of the record's state word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum State {
    Idle = 0,
    Open = 1,
    Closing = 2,
    CancelledOpen = 3,
    CancelledClose = 4,
}

/// The registry worker a record registers with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Worker(pub u32);

impl Worker {
    pub const CANCEL: Worker = Worker(2);
}

/// The message codes the machine posts to itself.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Message {
    Start = 0x2b01,
    Cancel = 0x2b18,
    Opened = 0x2b15,
    Positioned = 0x2b17,
    Written = 0x2c38,
    Closed = 0x2b16,
    Deleted = 0x2c3c,
}

/// The dispatcher's event numbers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Event {
    Start = 6,
    Cancel = 7,
    Opened = 15,
    Positioned = 16,
    Closed = 18,
    Written = 19,
    Deleted = 21,
}

/// The event table in its stock order.
pub const EVENTS: [(Message, Event); 7] = [
    (Message::Start, Event::Start),
    (Message::Cancel, Event::Cancel),
    (Message::Opened, Event::Opened),
    (Message::Positioned, Event::Positioned),
    (Message::Written, Event::Written),
    (Message::Closed, Event::Closed),
    (Message::Deleted, Event::Deleted),
];

/// The extension strings in their stock order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
pub enum Extension {
    Raf = 0,
    Jpg = 1,
    Mpo = 2,
    Raw = 3,
    Wav = 4,
}

pub const EXTENSIONS: [[u8; 8]; 5] = [*b".RAF\0\0\0\0", *b".JPG\0\0\0\0", *b".MPO\0\0\0\0", *b".RAW\0\0\0\0", *b".WAV\0\0\0\0"];

/// The descriptor's kind word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FileKind {
    Jpeg,
    Mpo,
    Raf,
    Raw,
    Named,
    Other,
}

impl FileKind {
    fn parse(word: u32) -> FileKind {
        match word {
            0 => FileKind::Jpeg,
            1 => FileKind::Mpo,
            3 => FileKind::Raf,
            6 => FileKind::Raw,
            7 => FileKind::Named,
            _ => FileKind::Other,
        }
    }

    fn extension(self) -> Extension {
        match self {
            FileKind::Jpeg => Extension::Jpg,
            FileKind::Mpo if cfg!(sfm_control_extension_swapped) => Extension::Raf,
            FileKind::Mpo => Extension::Mpo,
            FileKind::Raf if cfg!(sfm_control_extension_swapped) => Extension::Mpo,
            FileKind::Raf => Extension::Raf,
            FileKind::Raw => Extension::Raw,
            FileKind::Named | FileKind::Other => Extension::Wav,
        }
    }
}

/// The file service's open mode index.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum OpenMode {
    Create = 1,
    Append = 2,
}

/// A request to the file service; each carries the machine's callback for its result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Request {
    Open { name: u32, mode: OpenMode },
    Seek { handle: u32 },
    Write { handle: u32, size: u32, data: u32 },
    Close { handle: u32 },
    Release { handle: u32 },
    Delete { path: u32 },
}

/// The event code a failure reports through `0x726c74(1, code)`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
pub enum Fault {
    Open = 0x70,
    Transfer = 0x71,
    Close = 0x72,
}

/// A file-service completion status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Status(u32);

impl Status {
    const PREPARE: u32 = 3;

    fn failed(self) -> bool {
        self.0 != 0
    }

    /// The completion result of a failed open, seek or write; `other` is the result of any status
    /// but 3.
    fn result(self, other: i32) -> i32 {
        if self.0 == Status::PREPARE && !cfg!(sfm_control_status3_unmapped) {
            -4
        } else {
            other
        }
    }
}

/// The registry record a handler runs for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Task(pub u32);

pub trait Guest {
    fn r8(&self, address: u32) -> u8;
    fn r32(&self, address: u32) -> u32;
    fn w8(&mut self, address: u32, value: u8);
    fn w32(&mut self, address: u32, value: u32);
    /// `0x6e27ac` with the machine's event and state tables and the record as its own owner.
    fn register(&mut self, record: u32, worker: Worker);
    /// `0x6e2808`.
    fn unregister(&mut self, record: u32);
    /// `0x6e2b80(record, message, word2, word3, 0)`.
    fn post(&mut self, record: u32, message: Message, word2: u32, word3: u32);
    /// The file-service call for `request`; an open returns the handle.
    fn request(&mut self, request: Request) -> u32;
    fn report(&mut self, fault: Fault);
    /// `0x70fbfc(name, 0, prefix)`.
    fn prepare_name(&mut self, name: u32, prefix: Option<[u8; 5]>);
    /// `0x70f790`.
    fn set_prefix_mode(&mut self, mode: u32);
    /// `0x69f8fc`.
    fn property(&mut self, key: u32) -> u8;
    /// `strcpy`.
    fn copy_string(&mut self, destination: u32, source: u32);
    /// `strcat` of an extension string.
    fn append_extension(&mut self, destination: u32, extension: Extension);
    /// `memcpy`.
    fn copy_memory(&mut self, destination: u32, source: u32, length: u32);
    fn complete(&mut self, completion: u32, result: i32);
    fn run(&mut self, pending: u32);
}

fn owner(g: &impl Guest, task: Task) -> u32 {
    g.r32(task.0 + OWNER)
}

fn descriptor(g: &impl Guest, task: Task) -> u32 {
    g.r32(owner(g, task) + DESCRIPTOR)
}

fn status(g: &impl Guest, task: Task) -> Status {
    Status(g.r32(g.r32(task.0 + MESSAGE) + MESSAGE_STATUS))
}

fn set_state(g: &mut impl Guest, task: Task, state: State) {
    g.w32(task.0 + STATE, state as u32)
}

fn entry(descriptor: u32, index: u32) -> u32 {
    descriptor + ENTRIES + 8 * index
}

fn write_entry(g: &mut impl Guest, descriptor: u32, index: u32) {
    let at = entry(descriptor, index);
    let request = Request::Write { handle: g.r32(descriptor + HANDLE), size: g.r32(at + 4), data: g.r32(at) };
    g.request(request);
}

fn run_pending(g: &mut impl Guest) {
    let pending = g.r32(PENDING);
    if pending != 0 {
        g.run(pending);
        g.w32(PENDING, 0);
    }
}

fn fail(g: &mut impl Guest, task: Task, result: i32, fault: Fault) {
    let owner = owner(g, task);
    let completion = g.r32(owner + COMPLETION);
    set_state(g, task, State::Idle);
    g.unregister(task.0);
    g.w32(owner + RESULT, result as u32);
    g.report(fault);
    if completion != 0 {
        g.complete(completion, g.r32(owner + RESULT) as i32);
    }
}

// Entries -----------------------------------------------------------------------------------

/// `0x011d5600` (append 0) and `0x011d5688` (append 1).
pub fn begin(g: &mut impl Guest, worker: Worker, descriptor: u32, completion: u32, append: bool) {
    g.register(RECORD, worker);
    g.w32(descriptor + APPEND, append as u32);
    g.w32(RECORD + RESULT, 0);
    g.w32(RECORD + COMPLETION, completion);
    g.w32(RECORD + DESCRIPTOR, descriptor);
    g.post(RECORD, Message::Start, 0, 0);
}

/// `0x011d5714`.
pub fn request_cancel(g: &mut impl Guest, pending: u32) {
    if g.r32(RECORD + STATE) == State::Idle as u32 || cfg!(sfm_control_cancel_always_registers) {
        g.register(RECORD, Worker::CANCEL);
        g.w32(RECORD + RESULT, 0);
    }
    g.w32(PENDING, pending);
    g.post(RECORD, Message::Cancel, 0, 0);
}

/// `0x011d57ac`.
pub fn append_buffer(g: &mut impl Guest, descriptor: u32, data: u32, size: u32) -> i32 {
    for index in 0..ENTRY_COUNT {
        let at = entry(descriptor, index);
        if g.r32(at) != 0 || g.r32(at + 4) != 0 {
            continue;
        }
        if index > LAST_APPEND_SLOT {
            return -1;
        }
        g.w32(at, data);
        g.w32(at + 4, size);
        g.w32(at + 8, 0);
        g.w32(at + 12, 0);
        g.w32(descriptor + WRITE_INDEX, 0);
        return 0;
    }
    -1
}

/// `0x011d5830`.
pub fn output_mode(g: &impl Guest) -> u32 {
    g.r32(OUTPUT_MODE)
}

/// `0x011d5850`.
pub fn set_output_mode(g: &mut impl Guest, mode: u32) -> u32 {
    let prefix = match mode {
        0 | 1 => 0,
        2 => 1,
        3 if cfg!(sfm_control_output_mode_3_as_1) => 1,
        3 => 2,
        _ => return 0,
    };
    g.set_prefix_mode(prefix);
    g.w32(OUTPUT_MODE, mode);
    1
}

/// `0x011d58ac` (code) and `0x011d58e4` (version).
fn prefix(g: &mut impl Guest, first: u32) -> [u8; 5] {
    let mut text = [0; 5];
    for (index, byte) in text.iter_mut().take(4).enumerate() {
        *byte = g.property(first + index as u32);
    }
    text
}

/// `0x011d591c`.
pub fn copy_name(g: &mut impl Guest, buffer: u32, descriptor: u32) {
    g.copy_memory(buffer, descriptor + NAME, NAME_BYTES);
}

/// `0x011d5934`.
pub fn set_name(g: &mut impl Guest, text: u32, descriptor: u32) {
    let Some(cut) = (0..NAME_BYTES).find(|&index| g.r8(text + index) == b'.') else {
        return;
    };
    g.w8(text + cut, 0);
    if cut > LAST_CUT {
        return;
    }
    let extension = match g.r32(descriptor + KIND) {
        3 => Extension::Raf,
        0 => Extension::Jpg,
        _ => Extension::Mpo,
    };
    g.append_extension(text, extension);
    g.copy_memory(descriptor + NAME, text, NAME_BYTES);
    g.w32(descriptor + KIND, 7);
}

/// `0x011d59f4`.
fn build_name(g: &mut impl Guest, task: Task) {
    let file = descriptor(g, task);
    let source = if cfg!(sfm_control_prefix_version_for_source) { VERSION_PREFIX_PROPERTY } else { CODE_PREFIX_PROPERTY };
    let prefix = if g.r32(file + PREFIX_SOURCE) == 1 {
        Some(prefix(g, source))
    } else if output_mode(g) == 1 {
        Some(prefix(g, VERSION_PREFIX_PROPERTY))
    } else {
        None
    };
    g.prepare_name(file + NAME, prefix);
}

// Callbacks handed to the file service ------------------------------------------------------

/// `0x011d50d0`.
pub fn on_deleted(g: &mut impl Guest, status: u32) {
    g.post(RECORD, Message::Deleted, status, 0);
}

/// `0x011d5100`.
pub fn on_closed(g: &mut impl Guest, status: u32) {
    g.post(RECORD, Message::Closed, status, 0);
}

/// `0x011d5130`.
pub fn on_written(g: &mut impl Guest, status: u32, value: u32) {
    g.post(RECORD, Message::Written, status, value);
}

/// `0x011d5164`.
pub fn on_positioned(g: &mut impl Guest, status: u32, value: u32) {
    g.post(RECORD, Message::Positioned, status, value);
}

/// `0x011d5198`.
pub fn on_opened(g: &mut impl Guest, status: u32, value: u32) {
    g.post(RECORD, Message::Opened, status, value);
}

// Handlers --------------------------------------------------------------------------------------

/// Idle, start (`0x011d5aac`).
pub fn open(g: &mut impl Guest, task: Task) {
    let file = descriptor(g, task);
    g.w32(file + WRITE_INDEX, 0);
    let name = file + NAME;
    let kind = FileKind::parse(g.r32(file + KIND));
    let mode = if kind == FileKind::Named {
        OpenMode::Create
    } else if g.r32(file + APPEND) == 1 {
        if cfg!(sfm_control_append_mode_create) { OpenMode::Create } else { OpenMode::Append }
    } else {
        build_name(g, task);
        g.append_extension(name, kind.extension());
        OpenMode::Create
    };
    let handle = g.request(Request::Open { name, mode });
    g.w32(file + HANDLE, handle);
    set_state(g, task, State::Open);
}

/// Idle, cancel (`0x011d51f0`).
pub fn cancel_idle(g: &mut impl Guest, _: Task) {
    if g.r8(SAVED_PATH) != 0 {
        g.request(Request::Delete { path: SAVED_PATH });
    } else {
        run_pending(g);
    }
}

/// Idle, deleted (`0x011d52f4`).
pub fn deleted_idle(g: &mut impl Guest, task: Task) {
    g.w8(SAVED_PATH, 0);
    g.unregister(task.0);
    run_pending(g);
}

/// Open, opened (`0x011d5540`).
pub fn opened(g: &mut impl Guest, task: Task) {
    let file = descriptor(g, task);
    let status = status(g, task);
    if status.failed() {
        fail(g, task, status.result(-1), Fault::Open);
    } else if g.r32(file + APPEND) == 1 {
        g.request(Request::Seek { handle: g.r32(file + HANDLE) });
    } else {
        write_entry(g, file, g.r32(file + WRITE_INDEX));
    }
}

/// Open, positioned (`0x011d54b8`).
pub fn positioned(g: &mut impl Guest, task: Task) {
    let file = descriptor(g, task);
    let status = status(g, task);
    if status.failed() {
        fail(g, task, status.result(-2), Fault::Transfer);
    } else {
        write_entry(g, file, g.r32(file + WRITE_INDEX));
    }
}

/// Open, written (`0x011d5400`).
pub fn written(g: &mut impl Guest, task: Task) {
    let owner = owner(g, task);
    let file = g.r32(owner + DESCRIPTOR);
    let status = status(g, task);
    let handle = g.r32(file + HANDLE);
    if status.failed() {
        if cfg!(sfm_control_write_error_completes) {
            fail(g, task, status.result(-2), Fault::Transfer);
            return;
        }
        g.w32(owner + RESULT, status.result(-2) as u32);
        g.report(Fault::Transfer);
        g.request(Request::Release { handle });
        set_state(g, task, State::Closing);
        return;
    }
    let index = g.r32(file + WRITE_INDEX) + 1;
    if !cfg!(sfm_control_index_unadvanced) {
        g.w32(file + WRITE_INDEX, index);
    }
    let at = entry(file, index);
    let end = if cfg!(sfm_control_terminator_by_size) { g.r32(at + 4) == 0 } else { g.r32(at) == 0 };
    if end {
        g.request(Request::Close { handle });
        set_state(g, task, State::Closing);
    } else {
        g.request(Request::Write { handle, size: g.r32(at + 4), data: g.r32(at) });
    }
}

/// Open, cancel (`0x011d533c`).
pub fn cancel_open(g: &mut impl Guest, task: Task) {
    let file = descriptor(g, task);
    g.request(Request::Release { handle: g.r32(file + HANDLE) });
    set_state(g, task, State::CancelledOpen);
}

/// Closing, closed (`0x011d536c`).
pub fn closed(g: &mut impl Guest, task: Task) {
    let owner = owner(g, task);
    let file = g.r32(owner + DESCRIPTOR);
    if status(g, task).failed() {
        let completion = g.r32(owner + COMPLETION);
        set_state(g, task, State::Idle);
        g.unregister(task.0);
        g.w32(owner + RESULT, -3i32 as u32);
        g.report(Fault::Close);
        if completion != 0 {
            g.complete(completion, g.r32(owner + RESULT) as i32);
        }
        return;
    }
    if !cfg!(sfm_control_path_unsaved) {
        g.copy_string(SAVED_PATH, file + NAME);
    }
    set_state(g, task, State::Idle);
    let completion = g.r32(owner + COMPLETION);
    g.unregister(task.0);
    if completion != 0 {
        g.complete(completion, g.r32(owner + RESULT) as i32);
    }
}

/// Closing, cancel (`0x011d5090`).
pub fn cancel_closing(g: &mut impl Guest, task: Task) {
    set_state(g, task, State::CancelledClose);
}

/// Cancelled open, opened or written (`0x011d50ac`).
pub fn notify_owner(g: &mut impl Guest, task: Task) {
    let completion = g.r32(owner(g, task) + COMPLETION);
    if completion != 0 {
        g.complete(completion, 0);
    }
}

/// Cancelled open, closed (`0x011d5250`).
pub fn closed_after_cancel(g: &mut impl Guest, task: Task) {
    set_state(g, task, State::Idle);
    g.unregister(task.0);
    run_pending(g);
}

/// Cancelled close, closed (`0x011d51cc`).
pub fn delete_written(g: &mut impl Guest, task: Task) {
    let file = descriptor(g, task);
    g.request(Request::Delete { path: file + NAME });
}

/// Cancelled close, deleted (`0x011d5290`).
pub fn deleted_after_cancel(g: &mut impl Guest, task: Task) {
    let owner = owner(g, task);
    g.w8(SAVED_PATH, 0);
    set_state(g, task, State::Idle);
    let completion = g.r32(owner + COMPLETION);
    g.unregister(task.0);
    if completion != 0 {
        g.complete(completion, 0);
    }
    run_pending(g);
}
