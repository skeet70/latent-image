use core::num::NonZeroU32;
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};

macro_rules! word_type {
    ($($name:ident),+ $(,)?) => { $(
        #[derive(Clone, Copy)]
        #[repr(transparent)]
        struct $name(u32);
    )+ };
}
word_type!(BusAddress, ByteCount, Pixels, Coordinate, InterruptToken, ResourceHandle,
    EventWord, InterruptStatus, PackingWord, ScaleWord, EnableWord, RequestKind,
    StatusWord, CallbackWord, JobAddress, RecordAddress, ReservedLock, InterruptDescriptor,
    HandlerAddress, CacheArgument);

#[derive(Clone, Copy)]
#[repr(u32)]
enum CompletionStatus { Complete = 0, DeviceError = 1, OutputLimit = 2, SetupFailed = 3 }

#[derive(Clone, Copy)]
#[repr(transparent)]
struct ServiceResult(i32);
#[derive(Clone, Copy)]
#[repr(u32)]
enum WaitDeadline { Forever = u32::MAX }

#[derive(Clone, Copy)]
enum Packing { EightRowBlocks, SixteenRowBlocks }
impl Packing {
    fn from_abi(value: PackingWord) -> Self {
        if value.0 == 0 { Self::EightRowBlocks } else { Self::SixteenRowBlocks }
    }
    fn height_alignment(self) -> u32 {
        match self { Self::EightRowBlocks => 8, Self::SixteenRowBlocks => 16 }
    }
    fn device_word(self) -> PackingWord {
        PackingWord(match self { Self::EightRowBlocks => 1, Self::SixteenRowBlocks => 2 })
    }
}

#[derive(Clone, Copy)]
#[repr(u32)]
enum Downscale { Full = 0, Half = 1, Quarter = 2, Eighth = 3 }
impl Downscale {
    fn from_abi(value: ScaleWord) -> Self {
        match value.0 { 1 => Self::Half, 2 => Self::Quarter, 3 => Self::Eighth, _ => Self::Full }
    }
}

#[repr(C)]
struct Request {
    kind: RequestKind,
    packing: PackingWord,
    width: Pixels,
    height: Pixels,
    output_width: Pixels,
    output_height: Pixels,
    source: BusAddress,
    source_bytes: ByteCount,
    destination: BusAddress,
    produced_bytes: ByteCount,
    status: StatusWord,
    input_event: EventWord,
    output_event: EventWord,
    scale: ScaleWord,
    window_enabled: EnableWord,
    window_right: Coordinate,
    window_left: Coordinate,
    window_top: Coordinate,
    window_bottom: Coordinate,
    completed: CallbackWord,
    input_needed: CallbackWord,
}

#[repr(C)]
struct Configuration {
    source: BusAddress,
    source_bytes: ByteCount,
    destination: BusAddress,
    width: Pixels,
    height: Pixels,
    capacity: ByteCount,
    packing: PackingWord,
    scale: ScaleWord,
    window_enabled: EnableWord,
    window_left: Coordinate,
    window_right: Coordinate,
    window_top: Coordinate,
    window_bottom: Coordinate,
    tuning: [u32; 15],
}

// Lock 9 serializes requests. The registered IRQ accesses the current request without taking that lock.
const ACTIVE: *mut JobAddress = 0x014c_5888 as *mut JobAddress;
const IRQ_TOKEN: *mut InterruptToken = 0x014c_588c as *mut InterruptToken;
const CONFIGURATION: *mut Configuration = 0x014c_5890 as *mut Configuration;
const RESOURCE: *mut ResourceHandle = 0x014c_56f4 as *mut ResourceHandle;
const RECORD: *const RecordAddress = 0x014c_56f0 as *const RecordAddress;
const TRANSACTION_LOCK: ReservedLock = ReservedLock(9);
const INTERRUPT_DESCRIPTOR: InterruptDescriptor = InterruptDescriptor(0x48);
const FIRMWARE_INTERRUPT_ENTRY: HandlerAddress = HandlerAddress(0x00d9_9570);
const CACHE_ARGUMENT: CacheArgument = CacheArgument(0);

unsafe extern "C" {
    fn xm1_jpeg_wait(lock: ReservedLock, timeout: WaitDeadline) -> ServiceResult;
    fn xm1_jpeg_signal(lock: ReservedLock) -> ServiceResult;
    fn xm1_jpeg_resource_acquire(handle: *mut ResourceHandle) -> ServiceResult;
    fn xm1_jpeg_resource_release(handle: ResourceHandle);
    fn xm1_jpeg_reset();
    fn xm1_jpeg_program(configuration: *const Configuration);
    fn xm1_jpeg_start();
    fn xm1_jpeg_publish_restart(configuration: *const Configuration);
    fn xm1_jpeg_interrupt_acquire(descriptor: InterruptDescriptor, handler: HandlerAddress, token: *mut InterruptToken) -> ServiceResult;
    fn xm1_jpeg_interrupt_release(token: InterruptToken);
    fn xm1_jpeg_cache(first: CacheArgument, second: CacheArgument);
    fn xm1_jpeg_status_read(output: *mut InterruptStatus);
    fn xm1_jpeg_status_ack(status: InterruptStatus);
    fn xm1_jpeg_input_event_read(output: *mut EventWord);
    fn xm1_jpeg_input_event_ack(event: EventWord);
    fn xm1_jpeg_output_event_read(output: *mut EventWord);
    fn xm1_jpeg_output_event_ack(event: EventWord);
    fn xm1_jpeg_endpoint_read(output: *mut ByteCount);
}

macro_rules! read_field {
    ($pointer:expr, $field:ident) => {{
        let pointer = $pointer;
        unsafe { read_volatile(addr_of!((*pointer).$field)) }
    }};
}
macro_rules! write_field {
    ($pointer:expr, $field:ident, $value:expr) => {{
        let pointer = $pointer;
        let value = $value;
        unsafe { write_volatile(addr_of_mut!((*pointer).$field), value) }
    }};
}

#[path = "jpeg_device.rs"]
mod device;

unsafe fn current() -> *mut Request {
    unsafe { read_volatile(ACTIVE).0 as *mut Request }
}

unsafe fn callback(value: CallbackWord) {
    if let Some(address) = NonZeroU32::new(value.0) {
        let function: unsafe extern "C" fn() = unsafe { core::mem::transmute(address.get()) };
        unsafe { function() };
    }
}

fn align(value: u32, alignment: u32) -> u32 {
    value.wrapping_add(alignment - 1) & !(alignment - 1)
}

fn fatal() -> ! {
    loop { core::hint::spin_loop(); }
}

unsafe fn configure(request: *mut Request) -> ServiceResult {
    unsafe {
        write_volatile(ACTIVE, JobAddress(request as u32));
        xm1_jpeg_reset();
    }
    let request = unsafe { current() };
    let configuration = CONFIGURATION;
    let width = read_field!(request, width);
    let height = read_field!(request, height);
    write_field!(configuration, source, read_field!(request, source));
    write_field!(configuration, source_bytes, read_field!(request, source_bytes));
    write_field!(configuration, destination, read_field!(request, destination));
    write_field!(configuration, width, width);
    write_field!(configuration, height, height);
    let (output_width, output_height) = if read_field!(request, window_enabled).0 == 0 {
        write_field!(configuration, window_enabled, EnableWord(0));
        write_field!(configuration, window_left, Coordinate(0));
        write_field!(configuration, window_right, Coordinate(0));
        write_field!(configuration, window_top, Coordinate(0));
        write_field!(configuration, window_bottom, Coordinate(0));
        (width.0, height.0)
    } else {
        let right = read_field!(request, window_right);
        let left = read_field!(request, window_left);
        let top = read_field!(request, window_top);
        let bottom = read_field!(request, window_bottom);
        write_field!(configuration, window_enabled, EnableWord(1));
        write_field!(configuration, window_left, left);
        write_field!(configuration, window_right, right);
        write_field!(configuration, window_top, top);
        write_field!(configuration, window_bottom, bottom);
        (right.0.wrapping_add(1).wrapping_sub(left.0), bottom.0.wrapping_add(1).wrapping_sub(top.0))
    };
    let scale = Downscale::from_abi(read_field!(request, scale));
    let packing = Packing::from_abi(read_field!(request, packing));
    let height_alignment = packing.height_alignment();
    write_field!(request, output_width, Pixels(align(output_width >> scale as u32, 16)));
    write_field!(request, output_height, Pixels(align(output_height >> scale as u32, height_alignment)));
    write_field!(configuration, scale, ScaleWord(scale as u32));
    write_field!(configuration, packing, packing.device_word());
    let capacity = (width.0.wrapping_add(15) >> 4).wrapping_mul(32)
        .wrapping_mul(align(height.0, height_alignment));
    write_field!(configuration, capacity, ByteCount(capacity));
    let tuning = unsafe { addr_of_mut!((*configuration).tuning).cast::<u32>() };
    unsafe {
        write_volatile(tuning, 0);
        write_volatile(tuning.add(1), 0);
        write_volatile(tuning.add(2), 0);
        write_volatile(tuning.add(3), 0);
        write_volatile(tuning.add(4), 1);
        write_volatile(tuning.add(5), 0);
        write_volatile(tuning.add(6), 1);
        write_volatile(tuning.add(7), 0);
        write_volatile(tuning.add(8), 1);
        write_volatile(tuning.add(9), 1);
        write_volatile(tuning.add(10), 1);
        write_volatile(tuning.add(11), 1);
        write_volatile(tuning.add(12), 1);
        write_volatile(tuning.add(13), 1);
        write_volatile(tuning.add(14), 0);
    }
    write_field!(request, status, StatusWord(CompletionStatus::Complete as u32));
    write_field!(request, input_event, EventWord(0));
    write_field!(request, output_event, EventWord(0));
    write_field!(request, produced_bytes, ByteCount(0));
    unsafe { xm1_jpeg_program(configuration) };
    let result = unsafe { xm1_jpeg_interrupt_acquire(INTERRUPT_DESCRIPTOR, FIRMWARE_INTERRUPT_ENTRY, IRQ_TOKEN) };
    if result.0 != 0 { return result; }
    unsafe {
        xm1_jpeg_cache(CACHE_ARGUMENT, CACHE_ARGUMENT);
        xm1_jpeg_start();
    }
    ServiceResult(0)
}

unsafe fn finish() {
    unsafe { xm1_jpeg_reset() };
    let token = unsafe { read_volatile(IRQ_TOKEN) };
    if token.0 != 0 {
        unsafe {
            xm1_jpeg_interrupt_release(token);
            write_volatile(IRQ_TOKEN, InterruptToken(0));
        }
    }
}

/// # Safety
/// The address must name a four-byte-aligned writable 0x54-byte request that remains live through release. Nonzero callbacks must implement void(void).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_acquire(address: u32) {
    let request = address as *mut Request;
    if unsafe { xm1_jpeg_wait(TRANSACTION_LOCK, WaitDeadline::Forever).0 } != 0 { fatal(); }
    if unsafe { read_volatile(RESOURCE).0 } == 0
        && unsafe { xm1_jpeg_resource_acquire(RESOURCE).0 } != 0 { fatal(); }
    if unsafe { configure(request).0 } != 0 {
        write_field!(request, status, StatusWord(CompletionStatus::SetupFailed as u32));
        let on_complete = read_field!(request, completed);
        unsafe { callback(on_complete) };
    }
}

/// # Safety
/// A request must hold lock 9. The argument must cover the readable request layout, and the active request must remain writable. The retained endpoint-reprogram service must implement its source-bound completion wait.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_restart(address: u32) {
    let request = address as *const Request;
    write_field!(CONFIGURATION, source, read_field!(request, source));
    write_field!(CONFIGURATION, source_bytes, read_field!(request, source_bytes));
    let active = unsafe { current() };
    write_field!(active, status, StatusWord(CompletionStatus::Complete as u32));
    write_field!(active, input_event, EventWord(0));
    write_field!(active, output_event, EventWord(0));
    unsafe {
        xm1_jpeg_cache(CACHE_ARGUMENT, CACHE_ARGUMENT);
        xm1_jpeg_publish_restart(CONFIGURATION);
    }
}

/// # Safety
/// The caller must own lock 9. Reset and interrupt release must stop the current device transaction before its request storage is reused.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_release() {
    unsafe { finish() };
    let resource = unsafe { read_volatile(RESOURCE) };
    if resource.0 != 0 {
        unsafe {
            xm1_jpeg_resource_release(resource);
            write_volatile(RESOURCE, ResourceHandle(0));
        }
    }
    match unsafe { xm1_jpeg_signal(TRANSACTION_LOCK).0 } { 0 | -43 => (), _ => fatal() }
}

/// # Safety
/// Descriptor 0x48 must invoke this ordinary AAPCS callback only while the active request is live. Nonzero request callbacks must implement void(void). A nonzero record descriptor must cover bytes 0x35 and 0x36.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xm1_playback_jpeg_interrupt() {
    let mut status = core::mem::MaybeUninit::<InterruptStatus>::uninit();
    unsafe { xm1_jpeg_status_read(status.as_mut_ptr()) };
    let status = unsafe { status.assume_init() };
    unsafe { xm1_jpeg_status_ack(status) };
    if status.0 & (1 << 4) != 0 {
        write_field!(unsafe { current() }, status, StatusWord(CompletionStatus::Complete as u32));
        let mut event = core::mem::MaybeUninit::<EventWord>::uninit();
        unsafe { xm1_jpeg_output_event_read(event.as_mut_ptr()) };
        let event = unsafe { event.assume_init() };
        unsafe { xm1_jpeg_output_event_ack(event) };
        write_field!(unsafe { current() }, output_event, event);
        unsafe { xm1_jpeg_endpoint_read(addr_of_mut!((*current()).produced_bytes)) };
        let request = unsafe { current() };
        let endpoint = read_field!(request, produced_bytes);
        let destination = read_field!(request, destination);
        write_field!(request, produced_bytes, ByteCount(endpoint.0.wrapping_sub(destination.0)));
    } else if status.0 & (1 << 17) != 0 {
        write_field!(unsafe { current() }, status, StatusWord(CompletionStatus::OutputLimit as u32));
    } else if status.0 & (1 << 24) != 0 {
        let request = unsafe { current() };
        if read_field!(request, kind).0 == 1 {
            write_field!(request, status, StatusWord(CompletionStatus::Complete as u32));
            unsafe { update_record(true) };
            let on_input_needed = read_field!(request, input_needed);
            unsafe { callback(on_input_needed) };
        }
        return;
    } else {
        write_field!(unsafe { current() }, status, StatusWord(CompletionStatus::DeviceError as u32));
        let mut event = core::mem::MaybeUninit::<EventWord>::uninit();
        unsafe { xm1_jpeg_input_event_read(event.as_mut_ptr()) };
        let event = unsafe { event.assume_init() };
        unsafe { xm1_jpeg_input_event_ack(event) };
        write_field!(unsafe { current() }, input_event, event);
    }
    let complete = read_field!(unsafe { current() }, status).0 == CompletionStatus::Complete as u32;
    unsafe { update_record(complete) };
    let on_complete = read_field!(unsafe { current() }, completed);
    unsafe { callback(on_complete) };
}

unsafe fn update_record(complete: bool) {
    let record = unsafe { read_volatile(RECORD) };
    if record.0 != 0 {
        let pointer = record.0 as *mut u8;
        unsafe {
            write_volatile(pointer.add(0x35), if complete { 1 } else { 2 });
            write_volatile(pointer.add(0x36), if complete { 0 } else { 1 });
        }
    }
}
