//! The stock boundary of the request path: caller entries, posters, the result-slot
//! helpers, mediamsg's routing, the media task, status translation and media readiness.

use crate::completion::{self, Media, RawStatus, ReadinessMode, Translation};
use crate::requests::{self, Callback, Family, Payload, Posting, ResultSlot, Wait};
use crate::routing::{self, Header, Mediamsg, RequestCode, SessionClass, MEDIAMSG_TASK};
use crate::stock::{
    file_service_semaphore_signal as signal, file_service_semaphore_wait as wait, read_byte, read_half, read_word,
    write_byte, write_half, write_word, FOREVER,
};

const CONTEXT: u32 = 0x910204;
const CONTEXT_DEVICE: u32 = 0x114;
const CONTEXT_MEDIA_FLAGS: u32 = 0x16bd8;
const RESULT_OWNER: u32 = 0x910134;
const RESULT: u32 = 0x910138;
const RESULT_SEMAPHORE: u32 = 0x3a;
const DONE_SEMAPHORE: u32 = 0x3b;
const MEDIA_REPORT_CATEGORY: u32 = 2;
const MEDIA_BUSY: u32 = 0x940ac0;
const MEDIA_TICKS: u32 = 0x96bfb8;
const SESSION: u32 = 0x910218;
const OPEN_SET: u32 = 0x910290;
const DEFERRED: u32 = 0x9102e0;
const DEFERRED_BYTES: u32 = 96;
const INTERCEPTOR: u32 = 0x7217f0;
const MEDIA_TASK_ERROR: i32 = -63;

extern "C" {
    fn file_service_current_task() -> u32;
    fn file_service_local_record(record: *mut u32);
    fn file_service_local_open(path: u32) -> u32;
    fn file_service_local_position(position: u32);
    fn file_service_device_for_letter(letter: u32) -> u32;
    fn file_service_copy_string(destination: *mut u8, source: u32);
    fn file_service_copy_bytes(destination: u32, source: u32, length: u32);
    fn file_service_media_report(category: u32, code: u32);
    fn file_service_send(task: u32, header: u32) -> i32;
    fn file_service_receive_request(header: *mut u32) -> i32;
    fn file_service_sleep() -> i32;
    fn file_service_wake(task: u32) -> i32;
    fn file_service_ticks(destination: u32);
}

fn call2(function: u32, first: u32, second: u32) {
    let function: extern "C" fn(u32, u32) = unsafe { core::mem::transmute(function as usize) };
    function(first, second)
}

fn call1(function: u32, first: u32) {
    let function: extern "C" fn(u32) = unsafe { core::mem::transmute(function as usize) };
    function(first)
}

fn report(translation: Translation) -> u32 {
    if let Some(code) = translation.report {
        unsafe { file_service_media_report(MEDIA_REPORT_CATEGORY, code.0) };
    }
    translation.status.0
}

fn media() -> Media {
    let context = read_word(CONTEXT);
    if context == 0 || read_word(context + CONTEXT_DEVICE) == u32::MAX {
        Media::Absent
    } else {
        Media::Present { flags: read_byte(context + CONTEXT_MEDIA_FLAGS) }
    }
}

/// Entered at 0x71d450 by the 36 worker completions.
#[no_mangle]
pub extern "C" fn xm1_file_service_translate(_code: u32, raw: u32) -> u32 {
    report(completion::translate(RawStatus(raw)))
}

/// Entered at 0x71ddf0 by the posters and 40 other image-4 sites.
#[no_mangle]
pub extern "C" fn xm1_file_service_media_ready(mode: u32) -> u32 {
    report(completion::readiness(media(), ReadinessMode(mode)))
}

/// Entered at 0x71bc2c by the blocking posters: takes the result slot for this task.
#[no_mangle]
pub extern "C" fn xm1_file_service_result_acquire() -> u32 {
    if unsafe { wait(RESULT_SEMAPHORE, FOREVER) } != 0 {
        return 0;
    }
    write_word(RESULT_OWNER, unsafe { file_service_current_task() });
    RESULT
}

/// Entered at 0x71bc6c.
#[no_mangle]
pub extern "C" fn xm1_file_service_result_release() -> i32 {
    write_word(RESULT_OWNER, 0);
    unsafe { signal(RESULT_SEMAPHORE) }
}

/// Entered at 0x71bc90: waits for the worker's completion when this task owns the slot.
#[no_mangle]
pub extern "C" fn xm1_file_service_result_wait() -> i32 {
    if unsafe { file_service_current_task() } == read_word(RESULT_OWNER) {
        unsafe { wait(DONE_SEMAPHORE, FOREVER) }
    } else {
        MEDIA_TASK_ERROR
    }
}

/// Entered at 0x71c634 by the workers that complete a blocking request.
#[no_mangle]
pub extern "C" fn xm1_file_service_result_done() -> i32 {
    unsafe { signal(DONE_SEMAPHORE) }
}

/// Entered at 0x71fde4 by every poster: sends a copy of the header to mediamsg.
#[no_mangle]
pub extern "C" fn xm1_file_service_post(header: *const u32) -> i32 {
    let mut copy = [0u32; 3];
    for (index, word) in copy.iter_mut().enumerate() {
        *word = unsafe { core::ptr::read_volatile(header.add(index)) };
    }
    unsafe { file_service_send(u32::from(MEDIAMSG_TASK), copy.as_mut_ptr() as u32) }
}

struct StockPosting;

/// The flag, the slot and, for open, read and write, the slot's second word.
fn store_slot(at: u32, slot: Option<ResultSlot>, with_second: bool) {
    let (flag, address) = match slot {
        Some(ResultSlot(address)) => (1, address),
        None => (0, 0),
    };
    write_word(at, flag);
    write_word(at + 4, address);
    if with_second {
        write_word(at + 8, if flag == 0 { 0 } else { address + 4 });
    }
}

fn send_payload(code: RequestCode, source: u32, payload: u32, length: u32) {
    let header = [
        u32::from(code.0) | (source & 0xff) << 16 | u32::from(MEDIAMSG_TASK) << 24,
        length,
        payload,
    ];
    xm1_file_service_post(header.as_ptr());
}

impl Posting for StockPosting {
    fn current_task(&mut self) -> u32 {
        unsafe { file_service_current_task() }
    }

    fn local_mode(&mut self) -> u32 {
        let mut record = [0u32; 4];
        unsafe { file_service_local_record(record.as_mut_ptr()) };
        unsafe { core::ptr::read_volatile(record.as_ptr()) }
    }

    fn local_open(&mut self, path: u32) -> bool {
        unsafe { file_service_local_open(path) == 1 }
    }

    fn local_rewind(&mut self) {
        unsafe { file_service_local_position(0) }
    }

    fn readiness(&mut self, mode: ReadinessMode) -> Translation {
        completion::readiness(media(), mode)
    }

    fn path_byte(&self, path: u32, index: u32) -> u8 {
        read_byte(path + index)
    }

    fn device_for_letter(&mut self, letter: u8) -> u32 {
        unsafe { file_service_device_for_letter(u32::from(letter)) }
    }

    fn active_device(&self) -> Option<u32> {
        Some(read_word(read_word(CONTEXT) + CONTEXT_DEVICE))
    }

    fn allocate_tag(&mut self) -> u32 {
        crate::stock::xm1_file_service_tag_allocate()
    }

    fn acquire_result(&mut self) -> Option<ResultSlot> {
        match xm1_file_service_result_acquire() {
            0 => None,
            address => Some(ResultSlot(address)),
        }
    }

    fn wait_result(&mut self) {
        xm1_file_service_result_wait();
    }

    fn release_result(&mut self) {
        xm1_file_service_result_release();
    }

    fn result(&self, slot: ResultSlot) -> (u32, u32) {
        (read_word(slot.0), read_word(slot.0 + 4))
    }

    fn complete(&mut self, callback: Callback, status: u32, value: u32) {
        if callback.0 != 0 {
            call2(callback.0, status, value)
        }
    }

    fn complete_status(&mut self, callback: Callback, status: u32) {
        if callback.0 != 0 {
            call1(callback.0, status)
        }
    }

    fn post(&mut self, code: RequestCode, source: u32, payload: Payload) {
        match payload {
            Payload::Open { callback, path, mode, tag, slot, word } => {
                post_open(code, source, callback, path, mode, tag, slot, word)
            }
            Payload::Read { callback, handle, length: second, buffer: third, slot }
            | Payload::Write { callback, handle, second, third, slot } => {
                post_transfer(code, source, callback, handle, second, third, slot)
            }
            Payload::Close { callback, handle, word, name, slot } => {
                post_close(code, source, callback, handle, word, name, slot)
            }
        }
    }
}

/// The open payload, stored in stock's order: a path past 48 bytes overwrites the
/// result-slot words at +0x40, while +0x34..+0x40 and +0x4c are stored after it.
#[inline(never)]
#[link_section = ".text.file_service_post_open"]
#[allow(clippy::too_many_arguments)]
fn post_open(code: RequestCode, source: u32, callback: Callback, path: u32, mode: u32, tag: u32, slot: Option<ResultSlot>, word: u32) {
    let mut payload = core::mem::MaybeUninit::<[u32; 20]>::uninit();
    let at = payload.as_mut_ptr() as u32;
    store_slot(at + 0x40, slot, true);
    write_word(at, callback.0);
    unsafe { file_service_copy_string((at + 4) as *mut u8, path) };
    write_word(at + 0x38, mode);
    write_word(at + 0x34, 0);
    write_word(at + 0x3c, tag);
    write_word(at + 0x4c, word);
    send_payload(code, source, at, 0x50);
}

#[inline(never)]
#[link_section = ".text.file_service_post_transfer"]
fn post_transfer(code: RequestCode, source: u32, callback: Callback, handle: u32, second: u32, third: u32, slot: Option<ResultSlot>) {
    let mut payload = core::mem::MaybeUninit::<[u32; 7]>::uninit();
    let at = payload.as_mut_ptr() as u32;
    store_slot(at + 0x10, slot, true);
    write_word(at, callback.0);
    write_word(at + 4, handle);
    write_word(at + 8, second);
    write_word(at + 0xc, third);
    send_payload(code, source, at, 0x1c);
}

/// The close payload, stored in stock's order: a name longer than 20 bytes runs over the
/// result-slot words.
#[inline(never)]
#[link_section = ".text.file_service_post_close"]
fn post_close(code: RequestCode, source: u32, callback: Callback, handle: u32, word: u32, name: u32, slot: Option<ResultSlot>) {
    let mut payload = core::mem::MaybeUninit::<[u32; 10]>::uninit();
    let at = payload.as_mut_ptr() as u32;
    store_slot(at + 0x20, slot, false);
    write_word(at, callback.0);
    write_word(at + 4, handle);
    write_word(at + 8, word);
    if name == 0 {
        write_byte(at + 0xc, 0);
    } else {
        unsafe { file_service_copy_string((at + 0xc) as *mut u8, name) };
    }
    send_payload(code, source, at, 0x28);
}

/// Entered at 0x71c2b0 by the open entries and 0x71d030.
#[no_mangle]
pub extern "C" fn xm1_file_service_open(callback: u32, path: u32, mode: u32, wait: u32, word: u32, family: u32) -> u32 {
    requests::open(&mut StockPosting, Callback(callback), path, mode, Wait::from_word(wait), word, Family::from_word(family))
}

/// Entered at 0x71be74 by the read entries and 0x71d0a8.
#[no_mangle]
pub extern "C" fn xm1_file_service_read(callback: u32, handle: u32, length: u32, buffer: u32, wait: u32, family: u32) -> u32 {
    requests::read(&mut StockPosting, Callback(callback), handle, length, buffer, Wait::from_word(wait), Family::from_word(family))
}

/// Entered at 0x71bfe0 by the write entries and 0x71d080.
#[no_mangle]
pub extern "C" fn xm1_file_service_write(callback: u32, handle: u32, second: u32, third: u32, wait: u32, family: u32) -> u32 {
    requests::write(&mut StockPosting, Callback(callback), handle, second, third, Wait::from_word(wait), Family::from_word(family))
}

/// Entered at 0x71c134 by the close entries and 0x71d058.
#[no_mangle]
pub extern "C" fn xm1_file_service_close(callback: u32, handle: u32, word: u32, name: u32, wait: u32, family: u32) -> u32 {
    requests::close(&mut StockPosting, Callback(callback), handle, word, name, Wait::from_word(wait), Family::from_word(family))
}

/// 0x71c648.
#[no_mangle]
pub extern "C" fn xm1_file_service_open_async(callback: u32, path: u32, mode: u32) -> u32 {
    xm1_file_service_open(callback, path, mode, 0, 1, 0)
}

/// 0x71c670.
#[no_mangle]
pub extern "C" fn xm1_file_service_open_wait(path: u32, mode: u32) -> u32 {
    xm1_file_service_open(0, path, mode, 1, 1, 0)
}

/// 0x71c6a4.
#[no_mangle]
pub extern "C" fn xm1_file_service_open_unflagged(callback: u32, path: u32, mode: u32) -> u32 {
    xm1_file_service_open(callback, path, mode, 0, 0, 0)
}

/// 0x71c6c8.
#[no_mangle]
pub extern "C" fn xm1_file_service_open_unflagged_wait(path: u32, mode: u32) -> u32 {
    xm1_file_service_open(0, path, mode, 1, 0, 0)
}

/// 0x71c6fc.
#[no_mangle]
pub extern "C" fn xm1_file_service_read_async(callback: u32, handle: u32, length: u32, buffer: u32) -> u32 {
    xm1_file_service_read(callback, handle, length, buffer, 0, 0)
}

/// 0x71c720.
#[no_mangle]
pub extern "C" fn xm1_file_service_read_wait(handle: u32, length: u32, buffer: u32) -> u32 {
    xm1_file_service_read(0, handle, length, buffer, 1, 0)
}

/// 0x71c75c.
#[no_mangle]
pub extern "C" fn xm1_file_service_write_async(callback: u32, handle: u32, second: u32, third: u32) -> u32 {
    xm1_file_service_write(callback, handle, second, third, 0, 0)
}

/// 0x71c780.
#[no_mangle]
pub extern "C" fn xm1_file_service_write_wait(handle: u32, second: u32, third: u32) -> u32 {
    xm1_file_service_write(0, handle, second, third, 1, 0)
}

/// 0x71c7bc.
#[no_mangle]
pub extern "C" fn xm1_file_service_close_async(callback: u32, handle: u32, word: u32, name: u32) -> u32 {
    xm1_file_service_close(callback, handle, word, name, 0, 0)
}

/// 0x71c7e0.
#[no_mangle]
pub extern "C" fn xm1_file_service_close_wait(handle: u32, word: u32, name: u32) -> u32 {
    xm1_file_service_close(0, handle, word, name, 1, 0)
}

struct StockMediamsg;

impl Mediamsg for StockMediamsg {
    fn code(&self, header: Header) -> RequestCode {
        RequestCode(read_half(header.0))
    }

    fn set_code(&mut self, header: Header, code: RequestCode) {
        write_half(header.0, code.0)
    }

    fn set_route(&mut self, header: Header, source: u8, destination: u8) {
        write_byte(header.0 + 2, source);
        write_byte(header.0 + 3, destination);
    }

    fn payload(&self, header: Header) -> u32 {
        read_word(header.0 + 8)
    }

    fn word(&self, address: u32) -> u32 {
        read_word(address)
    }

    fn set_word(&mut self, address: u32, value: u32) {
        write_word(address, value)
    }

    fn class(&self) -> SessionClass {
        SessionClass(read_word(SESSION + 0x64))
    }

    fn set_class(&mut self, class: SessionClass) {
        write_word(SESSION + 0x64, class.0)
    }

    fn open_result(&self) -> (u32, u32) {
        (read_word(SESSION + 0x68), read_word(SESSION + 0x6c))
    }

    fn set_open_result(&mut self, status: u32, handle: u32) {
        write_word(SESSION + 0x68, status);
        write_word(SESSION + 0x6c, handle);
    }

    fn open_count(&self) -> u32 {
        read_word(SESSION + 0x70)
    }

    fn set_open_count(&mut self, count: u32) {
        write_word(SESSION + 0x70, count)
    }

    fn saved_callback(&self) -> u32 {
        read_word(SESSION + 0x74)
    }

    fn set_saved_callback(&mut self, callback: u32) {
        write_word(SESSION + 0x74, callback)
    }

    fn open_handle(&self, slot: usize) -> u32 {
        read_word(OPEN_SET + 4 * slot as u32)
    }

    fn set_open_handle(&mut self, slot: usize, handle: u32) {
        write_word(OPEN_SET + 4 * slot as u32, handle)
    }

    fn clear_open_set(&mut self) {
        for slot in 0..routing::OPEN_SET_SLOTS {
            self.set_open_handle(slot, 0);
        }
    }

    fn deferred_count(&self) -> u32 {
        read_word(SESSION + 0x60)
    }

    fn set_deferred_count(&mut self, count: u32) {
        write_word(SESSION + 0x60, count)
    }

    fn deferred_code(&self, slot: usize) -> RequestCode {
        RequestCode(read_half(DEFERRED + DEFERRED_BYTES * slot as u32))
    }

    fn store_deferred(&mut self, slot: usize, header: Header) {
        let entry = DEFERRED + DEFERRED_BYTES * slot as u32;
        unsafe { file_service_copy_bytes(entry, header.0, 12) };
        let length = read_word(header.0 + 4);
        if length != 0 {
            unsafe { file_service_copy_bytes(entry + 16, read_word(header.0 + 8), length) };
            write_word(entry + 8, entry + 16);
        }
    }

    fn replay(&mut self, slot: usize, route: fn(&mut Self, Header) -> i32) -> i32 {
        let entry = DEFERRED + DEFERRED_BYTES * slot as u32;
        let mut request = core::mem::MaybeUninit::<[u32; 24]>::uninit();
        let address = request.as_mut_ptr() as u32;
        let mut offset = 0;
        while offset < DEFERRED_BYTES {
            write_word(address + offset, read_word(entry + offset));
            write_word(entry + offset, 0);
            offset += 4;
        }
        write_word(address + 8, address + 16);
        route(self, Header(address))
    }

    fn interceptor(&self) -> u32 {
        INTERCEPTOR
    }

    fn send(&mut self, task: u8, header: Header) -> i32 {
        unsafe { file_service_send(u32::from(task), header.0) }
    }

    fn sleep(&mut self) -> i32 {
        unsafe { file_service_sleep() }
    }

    fn wake(&mut self, task: u8) {
        unsafe { file_service_wake(u32::from(task)) };
    }

    fn complete(&mut self, callback: u32, status: u32, handle: u32) {
        call2(callback, status, handle)
    }
}

/// Entered at 0x721578 by the mediamsg task.
#[no_mangle]
pub extern "C" fn xm1_file_service_route(header: u32) -> i32 {
    routing::route(&mut StockMediamsg, Header(header))
}

/// Installed by open routing as the open request's callback; entered by the open worker.
#[no_mangle]
pub extern "C" fn xm1_file_service_open_intercept(status: u32, handle: u32) {
    routing::intercept(&mut StockMediamsg, status, handle)
}

/// The media task body, entered at 0x71fe7c by the task created at 0x71fd40.
#[no_mangle]
pub extern "C" fn xm1_file_service_media_task() -> ! {
    unsafe { wait(DONE_SEMAPHORE, FOREVER) };
    let mut payload = [0u32; 21];
    let mut header = [0u32; 3];
    header[1] = 0x50;
    loop {
        unsafe { core::ptr::write_volatile(&mut header[2], payload.as_mut_ptr() as u32) };
        let received = unsafe { file_service_receive_request(header.as_mut_ptr()) };
        write_word(MEDIA_BUSY, 1);
        if received == 0 {
            let code = RequestCode(unsafe { core::ptr::read_volatile(header.as_ptr() as *const u16) });
            if let Some(worker) = routing::worker(code) {
                call1(worker, header.as_ptr() as u32);
            }
        }
        unsafe { file_service_ticks(MEDIA_TICKS) };
        write_word(MEDIA_BUSY, 0);
    }
}
