use core::{mem::MaybeUninit, ptr::{self, NonNull}, slice, sync::atomic::{AtomicBool, compiler_fence, Ordering}};

use crate::{
    CallbackResult, Clock, Completion, Publication, ReceiverKernel, SenderKernel,
    QUEUE_CAPACITY_BYTES, QUEUE_DEPTH, QUEUE_ID,
};
use crate::client::CompletionWords;
use crate::producer::NativeProducer;
use crate::protocol::{
    Callback, CallbackArgument, CommandKind, ProtocolError, Request, RequestId,
    COMMAND_COUNT, ENVELOPE_BYTES,
};
use crate::receiver::Receiver;
use crate::sender::Sender;
use xm1_application_nand::dma_native::{ClockBinding, ClockSource, CounterDirection, DrainContract};
use xm1_application_nand::dma_read::{Deadline, Instant};

const COMPLETION_LATCHES: u32 = 0x0094_4440;
const COMPLETION_SEMAPHORES: u32 = 0x0081_7700;
const PUBLIC_BUSY: u32 = 0x0095_7600;

#[repr(C)]
struct ChannelConfiguration { capacity_bytes: u32, depth: u32 }

#[repr(C)]
struct TaskConfiguration {
    channel: u32,
    name: *const u8,
    priority: u32,
    stack_bytes: u32,
    entry: unsafe extern "C" fn(*const u32),
    argument: *const u32,
    attributes: u32,
}

#[repr(C)]
struct Message {
    operation: u16,
    sender: u8,
    destination: u8,
    length: u32,
    data: *const u8,
}

extern "C" {
    fn xm1_loading_create_channel(channel: u32, configuration: *const ChannelConfiguration) -> i32;
    fn xm1_loading_create_task(configuration: *const TaskConfiguration) -> i32;
    fn xm1_loading_send_message(channel: u32, message: *const Message) -> i32;
    fn xm1_loading_receive_message(channel: u32, message: *mut Message, timeout: u32) -> i32;
    fn xm1_loading_current_task() -> u32;
    fn xm1_loading_wait_reserved(semaphore: u32, timeout: u32) -> i32;
    fn xm1_loading_signal_reserved(semaphore: u32) -> i32;
}

pub type DeadlineLeaf = unsafe extern "C" fn(*mut (), u32, *mut u64) -> i32;
pub type QuiescenceLeaf = unsafe extern "C" fn(*mut ()) -> u32;

#[derive(Clone, Copy)]
struct DeadlineProvider { context: *mut (), deadline: DeadlineLeaf }

#[repr(C)]
#[derive(Clone, Copy)]
pub struct LoadingConfiguration {
    pub version: u32,
    pub deadline_context: *mut (),
    pub deadline: Option<DeadlineLeaf>,
    /// 1 selects the pinned FujiMod 3d90035d synchronous-copy contract;
    /// 2 requires `quiescence` and its caller-owned context.
    pub drain_kind: u32,
    pub quiescence_context: *mut (),
    pub quiescence: Option<QuiescenceLeaf>,
}

struct NativeClock;
impl Clock for NativeClock {
    fn now(&mut self) -> Instant { CLOCK.now() }
}

struct NativeKernel;

static CLOCK: ClockBinding = unsafe { ClockBinding::new(CounterDirection::Up, ClockSource::NativeCore0) };
static TASK_NAME: &[u8; 15] = b"flashloader-rs\0";
static mut TASK_ARGUMENT: u32 = 0;
static mut CONFIGURATION: Option<(DeadlineProvider, DrainContract)> = None;
static mut SENDER: Option<Sender> = None;
static mut RECEIVER: Receiver = Receiver::new();
static mut PRODUCER: Option<NativeProducer> = None;
static SENDER_BORROWED: AtomicBool = AtomicBool::new(false);

struct SenderGuard;
impl SenderGuard {
    fn acquire() -> Result<Self, i32> {
        SENDER_BORROWED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map(|_| Self).map_err(|_| -2)
    }
}
impl Drop for SenderGuard {
    fn drop(&mut self) { SENDER_BORROWED.store(false, Ordering::Release); }
}
static mut COMPLETIONS: [Option<Completion>; COMMAND_COUNT] = [None; COMMAND_COUNT];

fn load(address: u32) -> u32 { unsafe { ptr::read_volatile(address as *const u32) } }
fn store(address: u32, value: u32) { unsafe { ptr::write_volatile(address as *mut u32, value) } }
fn status(code: i32) -> Result<(), ProtocolError> {
    if code == 0 { Ok(()) } else { Err(ProtocolError::Kernel(code)) }
}
fn external_status(error: ProtocolError) -> i32 {
    match error {
        ProtocolError::NotConfigured => -1,
        ProtocolError::Outstanding | ProtocolError::RetainedOperation => -2,
        ProtocolError::Expired => -3,
        ProtocolError::Kernel(code) | ProtocolError::Producer(code) if code != 0 => code,
        _ => -255,
    }
}

fn configured_deadline(kind: CommandKind) -> Result<Deadline, ProtocolError> {
    let Some((provider, _)) = (unsafe { CONFIGURATION }) else { return Err(ProtocolError::NotConfigured); };
    let mut count = 0u64;
    let code = unsafe { (provider.deadline)(provider.context, kind as u32, &mut count) };
    if code != 0 { return Err(ProtocolError::Kernel(code)); }
    let deadline = Deadline::new(Instant::new(count)).ok_or(ProtocolError::NonFiniteDeadline)?;
    if CLOCK.now() >= deadline.instant() { return Err(ProtocolError::Expired); }
    Ok(deadline)
}

impl SenderKernel for NativeKernel {
    fn initialize_queue(&mut self, queue: u32, capacity_bytes: u32, depth: u32) -> Result<(), ProtocolError> {
        if queue != QUEUE_ID || capacity_bytes != QUEUE_CAPACITY_BYTES || depth != QUEUE_DEPTH {
            return Err(ProtocolError::InvalidRequest);
        }
        let configuration = ChannelConfiguration { capacity_bytes, depth };
        status(unsafe { xm1_loading_create_channel(queue, &configuration) })
    }

    fn start_receiver(&mut self) -> Result<(), ProtocolError> {
        let configuration = TaskConfiguration {
            channel: QUEUE_ID,
            name: TASK_NAME.as_ptr(),
            priority: 4,
            stack_bytes: 0xc00,
            entry: xm1_loading_task,
            argument: ptr::addr_of!(TASK_ARGUMENT),
            attributes: 0,
        };
        status(unsafe { xm1_loading_create_task(&configuration) })
    }

    fn post(&mut self, queue: u32, operation: u16, envelope: &[u8; ENVELOPE_BYTES], deadline: Deadline) -> Result<(), ProtocolError> {
        if CLOCK.now() >= deadline.instant() { return Err(ProtocolError::Expired); }
        let message = Message {
            operation,
            sender: unsafe { xm1_loading_current_task() } as u8,
            destination: queue as u8,
            length: ENVELOPE_BYTES as u32,
            data: envelope.as_ptr(),
        };
        status(unsafe { xm1_loading_send_message(queue, &message) })
    }

    fn take_completion(&mut self, id: RequestId, kind: CommandKind) -> Option<Completion> {
        compiler_fence(Ordering::SeqCst);
        let slot = unsafe { &mut COMPLETIONS[kind.index()] };
        match *slot {
            Some(completion) if completion.id == id && completion.kind == kind => slot.take(),
            _ => None,
        }
    }

    fn wait(&mut self, _: RequestId, kind: CommandKind, deadline: Deadline) -> Result<(), ProtocolError> {
        if CLOCK.now() >= deadline.instant() { return Err(ProtocolError::Expired); }
        let semaphore = load(COMPLETION_SEMAPHORES + kind.index() as u32 * 4);
        if semaphore == 0 { return Err(ProtocolError::NotConfigured); }
        match unsafe { xm1_loading_wait_reserved(semaphore, 0) } {
            0 => status(unsafe { xm1_loading_signal_reserved(semaphore) }),
            -50 => Ok(()),
            code => Err(ProtocolError::Kernel(code)),
        }
    }
}

impl ReceiverKernel for NativeKernel {
    fn publish(&mut self, request: Request, publication: Publication) -> Result<(), ProtocolError> {
        if publication.callback_result == CallbackResult::Succeeded
            && CLOCK.now() >= request.deadline.instant()
        {
            return Err(ProtocolError::Expired);
        }
        unsafe { COMPLETIONS[request.kind.index()] = Some(publication.completion); }
        compiler_fence(Ordering::SeqCst);
        if publication.completed_latch {
            store(COMPLETION_LATCHES + request.kind.index() as u32 * 4, 1);
        }
        store(PUBLIC_BUSY, u32::from(publication.busy));
        let semaphore = load(COMPLETION_SEMAPHORES + request.kind.index() as u32 * 4);
        if semaphore != 0 { status(unsafe { xm1_loading_signal_reserved(semaphore) })?; }
        if let Callback::Address(address) = request.callback {
            let callback: unsafe extern "C" fn(u32, u32) =
                unsafe { core::mem::transmute(address.word() as usize) };
            unsafe { callback(request.argument.0, publication.callback_result.legacy_failed()); }
        }
        Ok(())
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_configure(configuration: *const LoadingConfiguration) -> i32 {
    if unsafe { SENDER.is_some() } { return -2; }
    let Some(configuration) = (unsafe { configuration.as_ref() }) else { return -30; };
    if configuration.version != 1 { return -30; }
    let Some(deadline) = configuration.deadline else { return -30; };
    let provider = DeadlineProvider { context: configuration.deadline_context, deadline };
    let drain = match configuration.drain_kind {
        1 => DrainContract::FujiMod3d90035d,
        2 => {
            let Some(context) = NonNull::new(configuration.quiescence_context) else { return -30; };
            let Some(quiescent) = configuration.quiescence else { return -30; };
            DrainContract::External { context, quiescent }
        }
        _ => return -30,
    };
    unsafe { CONFIGURATION = Some((provider, drain)); }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_initialize(argument: u32) -> i32 {
    if unsafe { SENDER.is_some() } { return 0; }
    let Some((_, drain)) = (unsafe { CONFIGURATION }) else { return -1; };
    unsafe {
        TASK_ARGUMENT = argument;
        RECEIVER = Receiver::new();
        COMPLETIONS = [None; COMMAND_COUNT];
        for index in 0..COMMAND_COUNT {
            store(COMPLETION_LATCHES + index as u32 * 4, 0);
        }
        store(PUBLIC_BUSY, 0);
        PRODUCER = Some(NativeProducer::new(&CLOCK, drain));
    }
    let mut kernel = NativeKernel;
    match Sender::new(&mut kernel) {
        Ok(sender) => { unsafe { SENDER = Some(sender); } 0 }
        Err(error) => external_status(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_request(
    kind: u32,
    deadline: u64,
    callback: u32,
    argument: u32,
) -> i32 {
    let Some(kind) = CommandKind::parse(kind) else { return -30; };
    let Some(deadline) = Deadline::new(Instant::new(deadline)) else { return -30; };
    let _guard = match SenderGuard::acquire() { Ok(guard) => guard, Err(code) => return code };
    let Some(sender) = (unsafe { SENDER.as_mut() }) else { return -1; };
    let callback = match Callback::parse(callback) { Some(callback) => callback, None => return -30 };
    let mut clock = NativeClock;
    let mut kernel = NativeKernel;
    match sender.send(kind, deadline, callback, CallbackArgument(argument), &mut clock, &mut kernel) {
        Ok(_) => 0,
        Err(error) => external_status(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_request_sync(kind: u32, deadline: u64, result: *mut CompletionWords) -> i32 {
    let Some(kind) = CommandKind::parse(kind) else { return -30; };
    let Some(deadline) = Deadline::new(Instant::new(deadline)) else { return -30; };
    if result.is_null() || (result as usize) & (core::mem::align_of::<CompletionWords>() - 1) != 0 { return -30; }
    let _guard = match SenderGuard::acquire() { Ok(guard) => guard, Err(code) => return code };
    let Some(sender) = (unsafe { SENDER.as_mut() }) else { return -1; };
    let mut clock = NativeClock;
    let mut kernel = NativeKernel;
    let completion = match sender.request_sync(kind, deadline, &mut clock, &mut kernel) {
        Ok(completion) => completion,
        Err(error) => return external_status(error),
    };
    let words = match CompletionWords::from_completion(completion) {
        Ok(words) => words,
        Err(error) => return external_status(error),
    };
    unsafe { result.write(words); }
    0
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_cancel(kind: u32, deadline: u64) -> i32 {
    let mut result = MaybeUninit::<CompletionWords>::uninit();
    unsafe { xm1_loading_request_sync(kind, deadline, result.as_mut_ptr()) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_absolute_deadline(kind: u32, result: *mut u64) -> i32 {
    let Some(kind) = CommandKind::parse(kind) else { return -30; };
    if result.is_null() || (result as usize) & (core::mem::align_of::<u64>() - 1) != 0 { return -30; }
    match configured_deadline(kind) {
        Ok(deadline) => { unsafe { result.write(deadline.instant().count()); } 0 }
        Err(error) => external_status(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_wait_channel(kind: u32, result: *mut CompletionWords) -> i32 {
    let Some(kind) = CommandKind::parse(kind) else { return -30; };
    if result.is_null() || (result as usize) & (core::mem::align_of::<CompletionWords>() - 1) != 0 { return -30; }
    let _guard = match SenderGuard::acquire() { Ok(guard) => guard, Err(code) => return code };
    let Some(sender) = (unsafe { SENDER.as_mut() }) else { return -1; };
    let Some(request) = sender.pending_for_kind(kind) else { return -30; };
    let mut clock = NativeClock;
    let mut kernel = NativeKernel;
    let completion = match sender.wait(request, &mut clock, &mut kernel) {
        Ok(completion) => completion,
        Err(error) => return external_status(error),
    };
    match CompletionWords::from_completion(completion) {
        Ok(words) => { unsafe { result.write(words); } 0 }
        Err(error) => external_status(error),
    }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_loading_task(_: *const u32) {
    loop {
        let mut message = Message { operation: 0, sender: 0, destination: 0, length: 0, data: ptr::null() };
        if unsafe { xm1_loading_receive_message(QUEUE_ID, &mut message, u32::MAX) } != 0 { continue; }
        if message.data.is_null() || message.length != ENVELOPE_BYTES as u32 {
            store(PUBLIC_BUSY, 1);
            continue;
        }
        let envelope = unsafe { slice::from_raw_parts(message.data, message.length as usize) };
        store(PUBLIC_BUSY, 1);
        let Some(producer) = (unsafe { PRODUCER.as_mut() }) else { continue; };
        let mut clock = NativeClock;
        let mut kernel = NativeKernel;
        let result = unsafe { RECEIVER.receive(message.operation, envelope, &mut clock, producer, &mut kernel) };
        if result.is_err() { store(PUBLIC_BUSY, 1); }
    }
}

const _: () = {
    assert!(core::mem::size_of::<ChannelConfiguration>() == 8);
    assert!(core::mem::size_of::<TaskConfiguration>() == 28);
    assert!(core::mem::size_of::<Message>() == 12);
    assert!(core::mem::size_of::<CompletionWords>() == 32);
};
