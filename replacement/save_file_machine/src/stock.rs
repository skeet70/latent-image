//! The retained services, the dispatcher tables and the functions the veneers at the stock
//! addresses and the state lists reach.

use crate::machine::{self, Event, Extension, Fault, Guest, Message, Request, Task, Worker, EVENTS, EXTENSIONS};
use core::ptr::{read_volatile, write_volatile};

extern "C" {
    fn sfm_register(record: u32, worker: u32, flags: u32, count: u32, state: u32, events: u32, states: u32, owner: u32);
    fn sfm_unregister(record: u32);
    fn sfm_post(record: u32, code: u32, word2: u32, word3: u32, word4: u32) -> u32;
    fn sfm_open(callback: u32, name: u32, mode: u32) -> u32;
    fn sfm_seek(callback: u32, handle: u32, low: u32, high: u32, origin: u32) -> u32;
    fn sfm_write(callback: u32, handle: u32, size: u32, data: u32) -> u32;
    fn sfm_close(callback: u32, handle: u32, word: u32, name: u32) -> u32;
    fn sfm_release(callback: u32, handle: u32) -> u32;
    fn sfm_delete(callback: u32, path: u32) -> u32;
    fn sfm_report(class: u32, code: u32);
    fn sfm_prepare_name(name: u32, flags: u32, prefix: u32) -> u32;
    fn sfm_set_prefix_mode(mode: u32);
    fn sfm_property(key: u32) -> u32;
    fn sfm_copy_string(destination: u32, source: u32) -> u32;
    fn sfm_append_string(destination: u32, source: u32) -> u32;
    fn sfm_copy_memory(destination: u32, source: u32, length: u32) -> u32;
}

const REGISTER_COUNT: u32 = 12;
const REPORT_CLASS: u32 = 1;

/// The callback addresses the veneers keep at their stock addresses.
const ON_DELETED: u32 = 0x011d_50d0;
const ON_CLOSED: u32 = 0x011d_5100;
const ON_WRITTEN: u32 = 0x011d_5130;
const ON_POSITIONED: u32 = if cfg!(sfm_control_seek_callback_open) { 0x011d_5198 } else { 0x011d_5164 };
const ON_OPENED: u32 = 0x011d_5198;

type Handler = extern "C" fn(u32);

#[repr(C)]
struct EventRow {
    code: u32,
    event: u32,
}

#[repr(C)]
struct Transition {
    event: u32,
    handler: Option<Handler>,
}

/// Read by the stock dispatcher `0x6e2aa0` through the record's `+0x10` and `+0x14` words.
#[repr(C)]
struct Tables {
    events: [EventRow; 8],
    extensions: [[u8; 8]; 5],
    states: [&'static Transition; 5],
}

const END: Transition = Transition { event: 91, handler: None };

const fn on(event: Event, handler: Handler) -> Transition {
    Transition { event: event as u32, handler: Some(handler) }
}

const fn row(index: usize) -> EventRow {
    EventRow { code: EVENTS[index].0 as u32, event: EVENTS[index].1 as u32 }
}

static IDLE: [Transition; 4] = [on(Event::Start, open), on(Event::Cancel, cancel_idle), on(Event::Deleted, deleted_idle), END];
static OPEN: [Transition; 5] =
    [on(Event::Opened, opened), on(Event::Positioned, positioned), on(Event::Written, written), on(Event::Cancel, cancel_open), END];
static CLOSING: [Transition; 3] = [on(Event::Closed, closed), on(Event::Cancel, cancel_closing), END];
static CANCELLED_OPEN: [Transition; 4] =
    [on(Event::Opened, notify_owner), on(Event::Written, notify_owner), on(Event::Closed, closed_after_cancel), END];
static CANCELLED_CLOSE: [Transition; 3] = [on(Event::Closed, delete_written), on(Event::Deleted, deleted_after_cancel), END];

#[link_section = ".rodata.sfm_tables"]
static TABLES: Tables = Tables {
    events: [row(0), row(1), row(2), row(3), row(4), row(5), row(6), EventRow { code: 0xffff, event: 0xffff_ffff }],
    extensions: EXTENSIONS,
    states: [&IDLE[0], &OPEN[0], &CLOSING[0], &CANCELLED_OPEN[0], &CANCELLED_CLOSE[0]],
};

fn events_address() -> u32 {
    &TABLES.events as *const _ as usize as u32
}

fn states_address() -> u32 {
    &TABLES.states as *const _ as usize as u32
}

struct Stock;

impl Guest for Stock {
    fn r8(&self, address: u32) -> u8 {
        unsafe { read_volatile(address as *const u8) }
    }
    fn r32(&self, address: u32) -> u32 {
        unsafe { read_volatile(address as *const u32) }
    }
    fn w8(&mut self, address: u32, value: u8) {
        unsafe { write_volatile(address as *mut u8, value) }
    }
    fn w32(&mut self, address: u32, value: u32) {
        unsafe { write_volatile(address as *mut u32, value) }
    }
    fn register(&mut self, record: u32, worker: Worker) {
        unsafe { sfm_register(record, worker.0, 0, REGISTER_COUNT, 0, events_address(), states_address(), record) }
    }
    fn unregister(&mut self, record: u32) {
        unsafe { sfm_unregister(record) }
    }
    fn post(&mut self, record: u32, message: Message, word2: u32, word3: u32) {
        unsafe { sfm_post(record, message as u32, word2, word3, 0) };
    }
    fn request(&mut self, request: Request) -> u32 {
        unsafe {
            match request {
                Request::Open { name, mode } => sfm_open(ON_OPENED, name, mode as u32),
                Request::Seek { handle } => sfm_seek(ON_POSITIONED, handle, 0, 0, 0),
                Request::Write { handle, size, data } => sfm_write(ON_WRITTEN, handle, size, data),
                Request::Close { handle } => sfm_close(ON_CLOSED, handle, 0, 0),
                Request::Release { handle } => sfm_release(ON_CLOSED, handle),
                Request::Delete { path } => sfm_delete(ON_DELETED, path),
            }
        }
    }
    fn report(&mut self, fault: Fault) {
        unsafe { sfm_report(REPORT_CLASS, fault as u32) }
    }
    fn prepare_name(&mut self, name: u32, prefix: Option<[u8; 5]>) {
        match prefix {
            Some(text) => unsafe { sfm_prepare_name(name, 0, text.as_ptr() as usize as u32) },
            None => unsafe { sfm_prepare_name(name, 0, 0) },
        };
    }
    fn set_prefix_mode(&mut self, mode: u32) {
        unsafe { sfm_set_prefix_mode(mode) }
    }
    fn property(&mut self, key: u32) -> u8 {
        unsafe { sfm_property(key) as u8 }
    }
    fn copy_string(&mut self, destination: u32, source: u32) {
        unsafe { sfm_copy_string(destination, source) };
    }
    fn append_extension(&mut self, destination: u32, extension: Extension) {
        let source = TABLES.extensions[extension as usize].as_ptr() as usize as u32;
        unsafe { sfm_append_string(destination, source) };
    }
    fn copy_memory(&mut self, destination: u32, source: u32, length: u32) {
        unsafe { sfm_copy_memory(destination, source, length) };
    }
    fn complete(&mut self, completion: u32, result: i32) {
        let function: extern "C" fn(i32) = unsafe { core::mem::transmute(completion as usize) };
        function(result)
    }
    fn run(&mut self, pending: u32) {
        let function: extern "C" fn() = unsafe { core::mem::transmute(pending as usize) };
        function()
    }
}

// Entries -------------------------------------------------------------------------------------

#[no_mangle]
pub extern "C" fn xm1_sfm_begin(worker: u32, descriptor: u32, completion: u32) {
    machine::begin(&mut Stock, Worker(worker), descriptor, completion, false)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_begin_append(worker: u32, descriptor: u32, completion: u32) {
    machine::begin(&mut Stock, Worker(worker), descriptor, completion, true)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_request_cancel(pending: u32) {
    machine::request_cancel(&mut Stock, pending)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_append_buffer(descriptor: u32, data: u32, size: u32) -> i32 {
    machine::append_buffer(&mut Stock, descriptor, data, size)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_set_output_mode(mode: u32) -> u32 {
    machine::set_output_mode(&mut Stock, mode)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_copy_name(buffer: u32, descriptor: u32) {
    machine::copy_name(&mut Stock, buffer, descriptor)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_set_name(text: u32, descriptor: u32) {
    machine::set_name(&mut Stock, text, descriptor)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_on_deleted(status: u32) {
    machine::on_deleted(&mut Stock, status)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_on_closed(status: u32) {
    machine::on_closed(&mut Stock, status)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_on_written(status: u32, value: u32) {
    machine::on_written(&mut Stock, status, value)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_on_positioned(status: u32, value: u32) {
    machine::on_positioned(&mut Stock, status, value)
}

#[no_mangle]
pub extern "C" fn xm1_sfm_on_opened(status: u32, value: u32) {
    machine::on_opened(&mut Stock, status, value)
}

// Handlers ------------------------------------------------------------------------------------

macro_rules! handlers {
    ($($name:ident),* $(,)?) => {$(
        extern "C" fn $name(task: u32) {
            machine::$name(&mut Stock, Task(task))
        }
    )*};
}

handlers!(
    open,
    cancel_idle,
    deleted_idle,
    opened,
    positioned,
    written,
    cancel_open,
    closed,
    cancel_closing,
    notify_owner,
    closed_after_cancel,
    delete_written,
    deleted_after_cancel,
);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
