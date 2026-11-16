use super::*;
use core::mem::MaybeUninit;

#[repr(transparent)]
struct RecordPixels(u16);
#[repr(transparent)]
struct RecordSelector(i8);
#[repr(transparent)]
struct RecordPacking(u8);

#[derive(Clone, Copy)]
#[repr(u32)]
enum BuilderKind { Default = 0, InputContinuation = 1, ModeEight = 2 }
impl BuilderKind {
    fn from_record(value: RecordSelector) -> Self {
        match value.0 { 7 => Self::InputContinuation, 8 => Self::ModeEight, _ => Self::Default }
    }
}

#[repr(C)]
struct RecordInputs {
    height: RecordPixels,
    width: RecordPixels,
    retained_04: [MaybeUninit<u8>; 8],
    destination: BusAddress,
    source: BusAddress,
    source_bytes: ByteCount,
    retained_18: [MaybeUninit<u8>; 28],
    selector: RecordSelector,
    retained_35: [MaybeUninit<u8>; 7],
    completed: CallbackWord,
    input_needed: CallbackWord,
    retained_44: [MaybeUninit<u8>; 32],
    packing: RecordPacking,
}

#[repr(C)]
struct Subscribers {
    completed: CallbackWord,
    input_needed: CallbackWord,
}
#[repr(transparent)]
struct CriticalState(u32);

const STATIC_REQUEST: *mut Request = 0x014c_56f8 as *mut Request;
const SUBSCRIBERS: *mut Subscribers = 0x014e_aed0 as *mut Subscribers;

unsafe extern "C" {
    fn xm1_jpeg_critical_enter() -> CriticalState;
    fn xm1_jpeg_critical_leave(state: CriticalState);
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct TimerHandle(u32);
#[repr(transparent)]
struct TimerMilliseconds(u32);
#[repr(transparent)]
struct TimerArgument(u32);
#[repr(transparent)]
struct PlaybackState(u32);
#[repr(transparent)]
struct PlaybackMode(u32);
#[repr(transparent)]
#[derive(Clone, Copy)]
struct DecodePixels(u16);

#[repr(C)]
struct DecodeRequirements {
    retained_00: [MaybeUninit<u8>; 8],
    kind: RequestKind,
    packing: PackingWord,
    width: DecodePixels,
    height: DecodePixels,
    source: BusAddress,
    source_bytes: ByteCount,
    destination: BusAddress,
    completed: CallbackWord,
    input_needed: CallbackWord,
}

#[repr(C)]
struct PlaybackRequest {
    request: Request,
    mode: PlaybackMode,
    state: PlaybackState,
    timer: TimerHandle,
    finished: u8,
    saved_prefix: [u8; 64],
    prefix_bytes: u8,
    retained_a2: [MaybeUninit<u8>; 2],
    subscribers: Subscribers,
}

const PLAYBACK: *mut PlaybackRequest = 0x014e_ae2c as *mut PlaybackRequest;

unsafe extern "C" {
    fn xm1_jpeg_timer_start(milliseconds: TimerMilliseconds, argument: TimerArgument, handler: CallbackWord, output: *mut TimerHandle) -> ServiceResult;
    fn xm1_jpeg_timer_cancel(handle: TimerHandle) -> ServiceResult;
}

/// # Safety
/// Requirements cover 40 readable bytes; source, output and subscribers remain live through cancellation or completion.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_initialize(address: u32) {
    let requirements = address as *const DecodeRequirements;
    let request = unsafe { addr_of_mut!((*PLAYBACK).request) };
    write_field!(request, kind, read_field!(requirements, kind));
    write_field!(request, packing, read_field!(requirements, packing));
    write_field!(request, width, Pixels(read_field!(requirements, width).0 as u32));
    write_field!(request, height, Pixels(read_field!(requirements, height).0 as u32));
    write_field!(request, source, read_field!(requirements, source));
    write_field!(request, source_bytes, read_field!(requirements, source_bytes));
    write_field!(request, destination, read_field!(requirements, destination));
    write_field!(request, status, StatusWord(0));
    write_field!(request, input_event, EventWord(0));
    write_field!(request, scale, ScaleWord(0));
    write_field!(request, window_enabled, EnableWord(0));
    write_field!(request, window_right, Coordinate(0));
    write_field!(request, window_left, Coordinate(0));
    write_field!(request, window_top, Coordinate(0));
    write_field!(request, window_bottom, Coordinate(0));
    write_field!(request, completed, CallbackWord(0x0121_fec8));
    write_field!(request, input_needed, CallbackWord(0x0121_ff20));
    write_field!(SUBSCRIBERS, completed, read_field!(requirements, completed));
    write_field!(SUBSCRIBERS, input_needed, read_field!(requirements, input_needed));
}

/// # Safety
/// The shared Playback request must be exclusively writable by this caller.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_reset_state() {
    write_field!(PLAYBACK, state, PlaybackState(0));
    write_field!(PLAYBACK, finished, 0);
}

unsafe fn start_timer() {
    unsafe {
        xm1_jpeg_timer_start(TimerMilliseconds(3000), TimerArgument(0),
            CallbackWord(0x0121_ab58), addr_of_mut!((*PLAYBACK).timer));
    }
}

/// # Safety
/// The initialized request and its resources remain live through the timer callback.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_restart_request() {
    unsafe {
        xm1_playback_jpeg_restart(PLAYBACK as u32);
        start_timer();
    }
}

/// # Safety
/// The timer handle belongs to the current Playback request.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_cancel_timer() {
    let handle = read_field!(PLAYBACK, timer);
    unsafe { xm1_jpeg_timer_cancel(handle) };
}

/// # Safety
/// The source has writable prefix capacity through its aligned base plus 33 bytes and remains live through restore. Preservation and restoration must not race another request.
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn xm1_playback_jpeg_preserve_prefix(restore: u32) {
    let request = unsafe { addr_of_mut!((*PLAYBACK).request) };
    if restore == 0 {
        let source = read_field!(request, source).0;
        let alignment = (source & 31) as u8;
        write_field!(PLAYBACK, prefix_bytes, alignment);
        if alignment == 0 { return; }
        write_field!(request, source_bytes, ByteCount(read_field!(request, source_bytes).0.wrapping_add(alignment as u32)));
        let aligned = source.wrapping_sub(alignment as u32);
        write_field!(request, source, BusAddress(aligned));
        write_field!(PLAYBACK, prefix_bytes, alignment + 2);
        let mut index = 0;
        while index < read_field!(PLAYBACK, prefix_bytes) {
            let pointer = aligned.wrapping_add(index as u32) as *mut u8;
            unsafe {
                write_volatile(addr_of_mut!((*PLAYBACK).saved_prefix).cast::<u8>().add(index as usize), read_volatile(pointer));
                write_volatile(pointer, match index { 0 => 0xff, 1 => 0xd8, _ => 0 });
            }
            index += 1;
        }
    } else {
        let source = read_field!(request, source).0;
        let mut index = 0;
        while index < read_field!(PLAYBACK, prefix_bytes) {
            unsafe {
                let saved = read_volatile(addr_of!((*PLAYBACK).saved_prefix).cast::<u8>().add(index as usize));
                write_volatile(source.wrapping_add(index as u32) as *mut u8, saved);
            }
            index += 1;
        }
    }
}

/// # Safety
/// The request owns the active channel and its source prefix is writable through restoration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_finish_request() {
    unsafe {
        xm1_playback_jpeg_cancel_timer();
        xm1_playback_jpeg_release();
        core::arch::asm!(".global xm1_playback_jpeg_finish_released", ".set xm1_playback_jpeg_finish_released, .", options(nostack, preserves_flags));
    }
    write_field!(PLAYBACK, finished, 1);
    unsafe {
        xm1_playback_jpeg_preserve_prefix(1);
        xm1_playback_jpeg_complete_first();
        xm1_playback_jpeg_complete_second();
    }
}

/// # Safety
/// The request owns the active channel and its source prefix is writable through restoration.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_cancel_request() {
    unsafe {
        xm1_playback_jpeg_cancel_timer();
        xm1_playback_jpeg_release();
        core::arch::asm!(".global xm1_playback_jpeg_cancel_released", ".set xm1_playback_jpeg_cancel_released, .", options(nostack, preserves_flags));
        xm1_playback_jpeg_preserve_prefix(1);
    }
}

/// # Safety
/// The initialized request owns a writable source prefix, output capacity and live subscribers until release.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_submit() {
    unsafe {
        xm1_playback_jpeg_preserve_prefix(0);
        xm1_playback_jpeg_acquire(PLAYBACK as u32);
        start_timer();
    }
}

/// # Safety
/// The aligned record must cover 0x65 readable bytes and remain live through release. Nonzero callbacks must implement void(void). Construction precedes lock 9 acquisition and must not race another constructor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_build_request(address: u32) {
    let record = address as *const RecordInputs;
    let request = STATIC_REQUEST;
    unsafe { write_volatile(RECORD, RecordAddress(address)) };
    write_field!(request, kind, RequestKind(0));
    match BuilderKind::from_record(read_field!(record, selector)) {
        BuilderKind::Default => (),
        kind => write_field!(request, kind, RequestKind(kind as u32)),
    }
    write_field!(request, packing, PackingWord(0));
    if read_field!(record, packing).0 == 1 {
        write_field!(request, packing, PackingWord(1));
    }
    write_field!(request, height, Pixels(read_field!(record, height).0 as u32));
    write_field!(request, width, Pixels(read_field!(record, width).0 as u32));
    write_field!(request, source, read_field!(record, source));
    write_field!(request, source_bytes, read_field!(record, source_bytes));
    write_field!(request, destination, read_field!(record, destination));
    write_field!(request, produced_bytes, ByteCount(0));
    write_field!(request, scale, ScaleWord(0));
    write_field!(request, window_enabled, EnableWord(0));
    write_field!(request, window_left, Coordinate(0));
    write_field!(request, window_right, Coordinate(0));
    write_field!(request, window_top, Coordinate(0));
    write_field!(request, window_bottom, Coordinate(0));
    write_field!(request, completed, read_field!(record, completed));
    write_field!(request, input_needed, read_field!(record, input_needed));
    unsafe { xm1_playback_jpeg_acquire(request as u32) };
}

/// # Safety
/// Published subscribers must implement void(void).
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn xm1_playback_jpeg_complete_first() {
    let state = unsafe { xm1_jpeg_critical_enter() };
    if read_field!(SUBSCRIBERS, input_needed).0 != 0 {
        let completed = read_field!(SUBSCRIBERS, completed);
        if completed.0 != 0 {
            write_field!(SUBSCRIBERS, completed, CallbackWord(0));
            unsafe {
                xm1_jpeg_critical_leave(state);
                callback(completed);
            }
            return;
        }
    }
    unsafe { xm1_jpeg_critical_leave(state) };
}

/// # Safety
/// Published subscribers must implement void(void).
#[unsafe(no_mangle)]
#[inline(never)]
pub unsafe extern "C" fn xm1_playback_jpeg_complete_second() {
    let state = unsafe { xm1_jpeg_critical_enter() };
    if read_field!(SUBSCRIBERS, completed).0 != 0 {
        let input_needed = read_field!(SUBSCRIBERS, input_needed);
        if input_needed.0 != 0 {
            write_field!(SUBSCRIBERS, input_needed, CallbackWord(0));
            unsafe {
                xm1_jpeg_critical_leave(state);
                callback(input_needed);
            }
            return;
        }
    }
    unsafe { xm1_jpeg_critical_leave(state) };
}

const _: () = assert!(core::mem::offset_of!(RecordInputs, selector) == 0x34);
const _: () = assert!(core::mem::offset_of!(RecordInputs, completed) == 0x3c);
const _: () = assert!(core::mem::offset_of!(RecordInputs, input_needed) == 0x40);
const _: () = assert!(core::mem::offset_of!(RecordInputs, packing) == 0x64);
const _: () = assert!(core::mem::offset_of!(PlaybackRequest, timer) == 0x5c);
const _: () = assert!(core::mem::offset_of!(PlaybackRequest, saved_prefix) == 0x61);
const _: () = assert!(core::mem::offset_of!(PlaybackRequest, prefix_bytes) == 0xa1);
const _: () = assert!(core::mem::offset_of!(PlaybackRequest, subscribers) == 0xa4);
