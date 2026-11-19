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

const _: () = assert!(core::mem::offset_of!(RecordInputs, selector) == 0x34);
const _: () = assert!(core::mem::offset_of!(RecordInputs, completed) == 0x3c);
const _: () = assert!(core::mem::offset_of!(RecordInputs, input_needed) == 0x40);
const _: () = assert!(core::mem::offset_of!(RecordInputs, packing) == 0x64);
