use core::ptr::{addr_of, read_volatile, write_volatile};
use super::{ByteCount, Configuration, Coordinate, EventWord, InterruptStatus};

#[derive(Clone, Copy)]
#[repr(transparent)]
struct RegisterWord(u32);

#[derive(Clone, Copy)]
#[repr(u32)]
enum Register {
    Control = 0xfff6_3000,
    Status = 0xfff6_3004,
    Mode = 0xfff6_3008,
    InputEvent = 0xfff6_3010,
    OutputEvent = 0xfff6_3014,
    Scale = 0xfff6_30a0,
    Destination = 0xfff6_3100,
    Capacity = 0xfff6_3104,
    Endpoint = 0xfff6_3108,
    Source = 0xfff6_310c,
    SourceBytes = 0xfff6_3110,
    Command = 0xfff6_3118,
    Format = 0xfff6_311c,
    WindowEnabled = 0xfff6_3120,
    HorizontalWindow = 0xfff6_3124,
    VerticalWindow = 0xfff6_3128,
}

impl Register {
    unsafe fn read(self) -> RegisterWord {
        RegisterWord(unsafe { read_volatile(self as u32 as *const u32) })
    }
    unsafe fn write(self, value: RegisterWord) {
        unsafe { write_volatile(self as u32 as *mut u32, value.0) };
    }
}

#[repr(transparent)]
struct WindowPair(RegisterWord);
impl WindowPair {
    fn from_abi(low: Coordinate, high: Coordinate) -> Self {
        if low.0 > u16::MAX as u32 || high.0 > u16::MAX as u32 { super::fatal(); }
        Self(RegisterWord(low.0 | (high.0 << 16)))
    }
}

unsafe fn tuning(configuration: *const Configuration, index: usize) -> RegisterWord {
    let words = unsafe { addr_of!((*configuration).tuning).cast::<u32>() };
    RegisterWord(unsafe { read_volatile(words.add(index)) })
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_reset() {
    unsafe { Register::Command.write(RegisterWord(1)) };
    let status = loop {
        let status = unsafe { Register::Status.read() };
        if status.0 & (1 << 16) != 0 { break status; }
        core::hint::spin_loop();
    };
    unsafe {
        Register::Status.write(status);
        Register::Control.write(RegisterWord(1 << 8));
    }
    while unsafe { Register::Control.read().0 } & (1 << 8) != 0 {
        core::hint::spin_loop();
    }
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_program(configuration: *const Configuration) {
    let mut mode = RegisterWord(0);
    for (index, bit) in [4, 5, 8, 16, 17, 18, 19, 20, 21, 22, 23, 24].into_iter().enumerate() {
        mode.0 |= (unsafe { tuning(configuration, index + 3).0 } & 1) << bit;
    }
    unsafe { Register::Mode.write(mode) };
    let scale = read_field!(configuration, scale);
    unsafe { Register::Scale.write(RegisterWord(scale.0 & 3)) };
    let destination = read_field!(configuration, destination);
    unsafe { Register::Destination.write(RegisterWord(destination.0)) };
    let capacity = read_field!(configuration, capacity);
    unsafe { Register::Capacity.write(RegisterWord(capacity.0)) };
    let source = read_field!(configuration, source);
    unsafe { Register::Source.write(RegisterWord(source.0)) };
    if !cfg!(xm1_jpeg_omit_source_bytes) {
        let count = read_field!(configuration, source_bytes);
        unsafe { Register::SourceBytes.write(RegisterWord(count.0)) };
    }
    let format = unsafe { RegisterWord((tuning(configuration, 0).0 & 15)
        | ((tuning(configuration, 1).0 & 1) << 4)
        | ((tuning(configuration, 2).0 & 1) << 5)) };
    unsafe { Register::Format.write(format) };
    let window_enabled = read_field!(configuration, window_enabled);
    unsafe { Register::WindowEnabled.write(RegisterWord(window_enabled.0 & 1)) };
    let horizontal = WindowPair::from_abi(read_field!(configuration, window_left), read_field!(configuration, window_right));
    unsafe { Register::HorizontalWindow.write(horizontal.0) };
    let vertical = WindowPair::from_abi(read_field!(configuration, window_top), read_field!(configuration, window_bottom));
    unsafe { Register::VerticalWindow.write(vertical.0) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_start() {
    let control = unsafe { Register::Control.read() };
    unsafe { Register::Control.write(RegisterWord(control.0 | 1)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_status_read(output: *mut InterruptStatus) {
    let value = unsafe { Register::Status.read() };
    unsafe { write_volatile(output, InterruptStatus(value.0)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_status_ack(status: InterruptStatus) {
    unsafe { Register::Status.write(RegisterWord(status.0)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_input_event_read(output: *mut EventWord) {
    let value = unsafe { Register::InputEvent.read() };
    unsafe { write_volatile(output, EventWord(value.0)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_input_event_ack(event: EventWord) {
    unsafe { Register::InputEvent.write(RegisterWord(event.0)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_output_event_read(output: *mut EventWord) {
    let value = unsafe { Register::OutputEvent.read() };
    unsafe { write_volatile(output, EventWord(value.0)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_output_event_ack(event: EventWord) {
    unsafe { Register::OutputEvent.write(RegisterWord(event.0)) };
}

#[inline(never)]
#[unsafe(no_mangle)]
pub(super) unsafe extern "C" fn xm1_jpeg_endpoint_read(output: *mut ByteCount) {
    let value = unsafe { Register::Endpoint.read() };
    unsafe { write_volatile(output, ByteCount(value.0)) };
}
