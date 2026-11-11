//! Task 99 CPU boundary. Platform entry addresses belong to both admitted RootPackages.
#![cfg(target_arch = "arm")]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use crate::control_actor::ControlConstructor;
use crate::control_callbacks::{NativeCallbackAddress, NativeCallbackRegistry, NativeCallbackUse};
use crate::control_types::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeDomain { A, B }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KernelSlot(u16);
impl KernelSlot {
    pub const fn new(value: u16) -> Option<Self> {
        if value > 0 && value <= 0x7fff { Some(Self(value)) } else { None }
    }
    pub const fn raw(self) -> u16 { self.0 }
    fn encoded(self, owner: NativeDomain) -> i32 {
        ((match owner { NativeDomain::A => 1, NativeDomain::B => 2 }) << 16) | i32::from(self.0)
    }
}

macro_rules! root {
    ($module:ident, $prefix:literal) => {
        mod $module {
            extern "C" {
                #[link_name = concat!($prefix, "queue_create")] pub fn queue_create(id: u32, packet: *const u32) -> i32;
                #[link_name = concat!($prefix, "queue_delete")] pub fn queue_delete(id: i32) -> i32;
                #[link_name = concat!($prefix, "send")] pub fn send(id: i32, value: u32) -> i32;
                #[link_name = concat!($prefix, "send_irq")] pub fn send_irq(id: i32, value: u32) -> i32;
                #[link_name = concat!($prefix, "receive")] pub fn receive(id: i32, value: *mut u32) -> i32;
                #[link_name = concat!($prefix, "task_create")] pub fn task_create(id: u32, packet: *const u32) -> i32;
                #[link_name = concat!($prefix, "task_delete")] pub fn task_delete(id: u32, mode: u32) -> i32;
                #[link_name = concat!($prefix, "activate")] pub fn activate(id: i32) -> i32;
                #[link_name = concat!($prefix, "delay")] pub fn delay(ticks: i32) -> i32;
                #[link_name = concat!($prefix, "cyclic_create")] pub fn cyclic_create(id: u32, packet: *const u32) -> i32;
                #[link_name = concat!($prefix, "cyclic_delete")] pub fn cyclic_delete(id: u32) -> i32;
                #[link_name = concat!($prefix, "cyclic_start")] pub fn cyclic_start(id: i32) -> i32;
                #[link_name = concat!($prefix, "cyclic_stop")] pub fn cyclic_stop(id: i32) -> i32;
            }
        }
    };
}
root!(a, "xm1_lens_root_a_");
root!(b, "xm1_lens_root_b_");
macro_rules! call {
    ($domain:expr, $method:ident $(, $arg:expr)* $(,)?) => {
        unsafe { match $domain { NativeDomain::A => a::$method($($arg),*), NativeDomain::B => b::$method($($arg),*) } }
    };
}
fn status(code: i32) -> Result<(), ControlError> {
    if code == 0 { Ok(()) } else { Err(ControlError::Queue) }
}
fn irq_context(entry: NativeDomain) -> bool {
    let address = match entry { NativeDomain::A => 0x4005_45a4, NativeDomain::B => 0x400e_4564 };
    unsafe { core::ptr::read_volatile(address as *const u32) != 0 }
}

#[derive(Clone, Copy)]
struct Envelope {
    message: ControlMessage,
    timer: Option<(&'static TimerSlot, u32)>,
}
struct MessageSlot {
    occupied: AtomicBool,
    value: UnsafeCell<MaybeUninit<Envelope>>,
}
impl MessageSlot {
    const fn new() -> Self { Self { occupied: AtomicBool::new(false), value: UnsafeCell::new(MaybeUninit::uninit()) } }
}
// A slot is written by its successful reserver and read only after kernel delivery.
unsafe impl Sync for MessageSlot {}

pub struct Task99Queue {
    owner: NativeDomain,
    id: KernelSlot,
    initialized: AtomicBool,
    slots: [MessageSlot; 32],
    ring: UnsafeCell<[u32; 32]>,
}
unsafe impl Sync for Task99Queue {}
impl Task99Queue {
    pub const fn new(owner: NativeDomain, id: KernelSlot) -> Self {
        Self { owner, id, initialized: AtomicBool::new(false), slots: [const { MessageSlot::new() }; 32], ring: UnsafeCell::new([0; 32]) }
    }
    pub fn sender(&self, entry: NativeDomain) -> QueueSender<'_> { QueueSender { queue: self, entry } }
    pub const fn slot(&self) -> KernelSlot { self.id }
    /// # Safety
    /// Exactly one task owns the returned receiver for this queue's lifetime.
    pub unsafe fn receiver(&self, entry: NativeDomain) -> NativeReceiver<'_> { NativeReceiver { queue: self, entry } }
    fn initialize(&self) -> Result<(), ControlError> {
        if self.initialized.load(Ordering::Acquire) { return Err(ControlError::Constructor); }
        let packet = [0, 32, self.ring.get() as u32];
        status(call!(self.owner, queue_create, u32::from(self.id.0), packet.as_ptr()))?;
        self.initialized.store(true, Ordering::Release);
        Ok(())
    }
}
#[derive(Clone, Copy)]
pub struct QueueSender<'a> { queue: &'a Task99Queue, entry: NativeDomain }
impl QueueSender<'_> {
    pub fn post(&self, message: ControlMessage) -> Result<(), ControlError> { self.enqueue(Envelope { message, timer: None }) }
    fn enqueue(&self, value: Envelope) -> Result<(), ControlError> {
        if !self.queue.initialized.load(Ordering::Acquire) { return Err(ControlError::Queue); }
        for (index, slot) in self.queue.slots.iter().enumerate() {
            if slot.occupied.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() { continue; }
            unsafe { (*slot.value.get()).write(value); }
            core::sync::atomic::fence(Ordering::Release);
            let id = self.queue.id.encoded(self.queue.owner);
            let result = if irq_context(self.entry) { call!(self.entry, send_irq, id, index as u32) } else { call!(self.entry, send, id, index as u32) };
            if result != 0 { slot.occupied.store(false, Ordering::Release); }
            return status(result);
        }
        Err(ControlError::Busy)
    }
}
pub struct NativeReceiver<'a> { queue: &'a Task99Queue, entry: NativeDomain }
impl NativeReceiver<'_> {
    pub fn receive_now(&mut self) -> Result<ControlMessage, ControlError> {
        loop {
            let mut index = 0;
            status(call!(self.entry, receive, self.queue.id.encoded(self.queue.owner), &mut index))?;
            let Some(slot) = self.queue.slots.get(index as usize) else { return Err(ControlError::InvalidPayload); };
            core::sync::atomic::fence(Ordering::Acquire);
            let value = unsafe { (*slot.value.get()).assume_init_read() };
            slot.occupied.store(false, Ordering::Release);
            if let Some((timer, generation)) = value.timer {
                if timer.generation.load(Ordering::Acquire) != generation { continue; }
                if !timer.repeating.load(Ordering::Acquire) { timer.cancel(); }
            }
            return Ok(value.message);
        }
    }
}

extern "C" { fn xm1_lens_task99_entry(argument: u32) -> !; }
#[repr(C, align(8))]
pub struct TaskStack(pub [u8; 3072]);
pub struct NativeConstructor {
    queue: &'static Task99Queue,
    task: KernelSlot,
    entry_domain: NativeDomain,
    stack: &'static mut TaskStack,
    argument: u32,
}
impl NativeConstructor {
    /// # Safety
    /// The task slot, queue slot and stack are reserved in the selected RootPackage.
    pub unsafe fn new(queue: &'static Task99Queue, task: KernelSlot, entry_domain: NativeDomain, stack: &'static mut TaskStack, argument: u32) -> Self {
        Self { queue, task, entry_domain, stack, argument }
    }
}
impl ControlConstructor for NativeConstructor {
    fn create_queue(&mut self, task: u8, bytes: usize, capacity: usize) -> Result<(), ControlError> {
        if (task, bytes, capacity) != (99, 48, 32) { return Err(ControlError::Constructor); }
        self.queue.initialize()
    }
    fn create_task(&mut self, task: u8, priority: u8, stack: usize) -> Result<(), ControlError> {
        if (task, priority, stack) != (99, 4, 3072) { return Err(ControlError::Constructor); }
        let domain = self.queue.owner;
        let packet = [2, self.argument, xm1_lens_task99_entry as usize as u32, 4, 3072, self.stack.0.as_mut_ptr() as u32];
        if call!(domain, task_create, u32::from(self.task.0), packet.as_ptr()) != 0 {
            call!(domain, queue_delete, i32::from(self.queue.id.0));
            self.queue.initialized.store(false, Ordering::Release);
            return Err(ControlError::Constructor);
        }
        if call!(self.entry_domain, activate, self.task.encoded(domain)) != 0 {
            call!(domain, task_delete, u32::from(self.task.0), 0);
            call!(domain, queue_delete, i32::from(self.queue.id.0));
            self.queue.initialized.store(false, Ordering::Release);
            return Err(ControlError::Constructor);
        }
        Ok(())
    }
}

static NEXT_TIMER_GENERATION: AtomicU32 = AtomicU32::new(1);
pub struct TimerSlot {
    sender: QueueSender<'static>,
    id: KernelSlot,
    kind: ControlTimer,
    generation: AtomicU32,
    repeating: AtomicBool,
    pending: AtomicBool,
    created: AtomicBool,
}
impl TimerSlot {
    pub const fn new(queue: &'static Task99Queue, domain: NativeDomain, id: KernelSlot, kind: ControlTimer) -> Self {
        Self { sender: QueueSender { queue, entry: domain }, id, kind, generation: AtomicU32::new(0), repeating: AtomicBool::new(false), pending: AtomicBool::new(false), created: AtomicBool::new(false) }
    }
    pub const fn slot(&self) -> KernelSlot { self.id }
    fn cancel(&self) {
        self.pending.store(false, Ordering::Release);
        self.generation.store(0, Ordering::Release);
        if self.created.swap(false, Ordering::AcqRel) {
            let domain = self.sender.entry;
            require(call!(domain, cyclic_stop, self.id.encoded(domain)));
            require(call!(domain, cyclic_delete, u32::from(self.id.0)));
        }
    }
    fn start(&'static self, ticks: TickCount, repeating: bool) -> TimerToken {
        self.cancel();
        let token = NEXT_TIMER_GENERATION.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| value.checked_add(1)).expect("task99 timer token exhausted");
        self.generation.store(token, Ordering::Release);
        self.repeating.store(repeating, Ordering::Release);
        self.pending.store(true, Ordering::Release);
        let packet = [0, self as *const Self as u32, xm1_lens_task99_timer as usize as u32, ticks.0, ticks.0];
        let domain = self.sender.entry;
        require(call!(domain, cyclic_create, u32::from(self.id.0), packet.as_ptr()));
        self.created.store(true, Ordering::Release);
        require(call!(domain, cyclic_start, self.id.encoded(domain)));
        TimerToken(token)
    }
}
fn require(code: i32) { assert!(code == 0, "task99 platform failure"); }
/// # Safety
/// `argument` points to a static TimerSlot installed by TimerSlot::start.
#[no_mangle]
pub unsafe extern "C" fn xm1_lens_task99_timer(argument: u32) {
    let timer = unsafe { &*(argument as *const TimerSlot) };
    if !timer.pending.load(Ordering::Acquire) { return; }
    let generation = timer.generation.load(Ordering::Acquire);
    let message = ControlMessage { opcode: ControlOpcode::Poll, payload: ControlPayload::Timer { kind: timer.kind, token: TimerToken(generation) } };
    assert!(timer.sender.enqueue(Envelope { message, timer: Some((timer, generation)) }).is_ok(), "task99 timer queue full");
    if !timer.repeating.load(Ordering::Acquire) { timer.pending.store(false, Ordering::Release); }
}

pub trait NativeEffects {
    fn complete(&mut self, callback: CallbackToken, status: CompletionStatus, data: &[u8]);
    fn notify(&mut self, target: WaitTarget);
    fn key_release(&mut self, event: KeyEvent);
    fn diagnostic(&mut self, category: u32, event: u32);
    fn set_detection_callback(&mut self, edge: DetectionEdge, enabled: bool);
    fn reset_gpio(&mut self);
}
pub struct NativeServices<E> {
    pub sender: QueueSender<'static>,
    pub timers: [&'static TimerSlot; 5],
    pub effects: E,
}
impl<E: NativeEffects> ControlServices for NativeServices<E> {
    fn delay_ticks(&mut self, ticks: TickCount) { require(call!(self.sender.entry, delay, ticks.0 as i32)); }
    fn post(&mut self, message: ControlMessage) -> Result<(), ControlError> { self.sender.post(message) }
    fn complete(&mut self, token: CallbackToken, status: CompletionStatus, data: &[u8]) { self.effects.complete(token, status, data); }
    fn notify(&mut self, target: WaitTarget) { self.effects.notify(target); }
    fn key_release(&mut self, event: KeyEvent) { self.effects.key_release(event); }
    fn diagnostic(&mut self, category: u32, event: u32) { self.effects.diagnostic(category, event); }
    fn start_timer(&mut self, ticks: TickCount, repeating: bool, callback: ControlTimer) -> TimerToken {
        self.timers.iter().find(|timer| timer.kind == callback).expect("reserved task99 timer").start(ticks, repeating)
    }
    fn cancel_timer(&mut self, token: TimerToken) {
        if token.0 != 0 {
            if let Some(timer) = self.timers.iter().find(|timer| timer.generation.load(Ordering::Acquire) == token.0) { timer.cancel(); }
        }
    }
    fn set_detection_callback(&mut self, edge: DetectionEdge, enabled: bool) { self.effects.set_detection_callback(edge, enabled); }
    fn reset_gpio(&mut self) { self.effects.reset_gpio(); }
}

pub struct CpuControlIo<S, P, R, K> { pub services: S, pub power: P, pub profiles: R, pub keys: K }
impl<S: ControlServices, P, R, K> ControlServices for CpuControlIo<S, P, R, K> {
    fn delay_ticks(&mut self, ticks: TickCount) { self.services.delay_ticks(ticks); }
    fn post(&mut self, message: ControlMessage) -> Result<(), ControlError> { self.services.post(message) }
    fn complete(&mut self, token: CallbackToken, status: CompletionStatus, data: &[u8]) { self.services.complete(token, status, data); }
    fn notify(&mut self, target: WaitTarget) { self.services.notify(target); }
    fn key_release(&mut self, event: KeyEvent) { self.services.key_release(event); }
    fn diagnostic(&mut self, category: u32, event: u32) { self.services.diagnostic(category, event); }
    fn start_timer(&mut self, ticks: TickCount, repeating: bool, callback: ControlTimer) -> TimerToken { self.services.start_timer(ticks, repeating, callback) }
    fn cancel_timer(&mut self, token: TimerToken) { self.services.cancel_timer(token); }
    fn set_detection_callback(&mut self, edge: DetectionEdge, enabled: bool) { self.services.set_detection_callback(edge, enabled); }
    fn reset_gpio(&mut self) { self.services.reset_gpio(); }
}
impl<S, P: ControlPower, R, K> ControlPower for CpuControlIo<S, P, R, K> {
    fn set_detection_mode(&mut self, mode: DetectionMode) { self.power.set_detection_mode(mode); }
    fn detection_output(&mut self, mode: DetectionMode, completion: Option<DetectionCompletion>) { self.power.detection_output(mode, completion); }
}
impl<S, P, R: ControlProfiles, K> ControlProfiles for CpuControlIo<S, P, R, K> {
    fn calibration_profiles(&self) -> &[crate::control_device::CalibrationProfile] { self.profiles.calibration_profiles() }
}
impl<S, P, R, K: KeyPublicationSink> KeyPublicationSink for CpuControlIo<S, P, R, K> {
    fn publish_key(&mut self, publication: crate::control_device::KeyPublication) { self.keys.publish_key(publication); }
}
impl<S: ControlServices, P: ControlPower, R: ControlProfiles, K: KeyPublicationSink> ControlIo for CpuControlIo<S, P, R, K> {
    fn read_mmio(&mut self, address: MmioAddress) -> u32 { unsafe { core::ptr::read_volatile(address.0 as *const u32) } }
    fn write_mmio(&mut self, address: MmioAddress, value: u32) { unsafe { core::ptr::write_volatile(address.0 as *mut u32, value); } }
    fn update_mmio(&mut self, address: MmioAddress, clear: u32, set: u32) {
        let saved = crate::wire_native::enter_critical();
        let value = self.read_mmio(address);
        self.write_mmio(address, (value & !clear) | set);
        crate::wire_native::leave_critical(saved);
    }
}

impl crate::control_runtime::ControlSender for QueueSender<'_> {
    fn post(&self, message: ControlMessage) -> Result<(), ControlError> { QueueSender::post(self, message) }
}

fn callback(registry: &impl NativeCallbackRegistry, address: u32, usage: NativeCallbackUse) -> Result<Option<CallbackToken>, ControlError> {
    if address == 0 { Ok(None) } else { registry.register(NativeCallbackAddress(address), usage).map(Some) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NativeMemoryAddress(pub u32);
pub trait NativeMemory: Sync {
    fn calibration_page(&self, address: NativeMemoryAddress) -> Result<CalibrationPage, ControlError>;
}
pub struct AdmittedNativeMemory<'a> { regions: &'a [&'a [u8]] }
impl<'a> AdmittedNativeMemory<'a> {
    pub const fn new(regions: &'a [&'a [u8]]) -> Self { Self { regions } }
}
impl NativeMemory for AdmittedNativeMemory<'_> {
    fn calibration_page(&self, address: NativeMemoryAddress) -> Result<CalibrationPage, ControlError> {
        for region in self.regions {
            let Some(offset) = address.0.checked_sub(region.as_ptr() as u32) else { continue; };
            let Some(end) = (offset as usize).checked_add(128) else { continue; };
            if let Some(source) = region.get(offset as usize..end) {
                let mut page = [0; 128];
                page.copy_from_slice(source);
                return Ok(CalibrationPage(page));
            }
        }
        Err(ControlError::InvalidPayload)
    }
}

// The retained message envelope is opcode:u16, sender:u8, destination:u8, length:u32, payload:u32.
pub fn decode_message(opcode: u16, payload: &[u8], registry: &impl NativeCallbackRegistry, memory: &(impl NativeMemory + ?Sized)) -> Result<ControlMessage, ControlError> {
    let opcode = ControlOpcode::try_from(opcode)?;
    if !payload.is_empty() && payload.len() != 24 { return Err(ControlError::InvalidPayload); }
    let word = |index: usize| -> Result<u32, ControlError> {
        let bytes = payload.get(index * 4..index * 4 + 4).ok_or(ControlError::InvalidPayload)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    };
    use ControlOpcode::*;
    let payload = match opcode {
        Register => ControlPayload::Register { enabled: word(0)? != 0, completion: callback(registry, word(1)?, NativeCallbackUse::Register)? },
        Setup => ControlPayload::Setup(SetupRequest {
            completion: callback(registry, word(4)?, NativeCallbackUse::Setup)?,
            polling: word(2)? != 0,
            mode: if word(1)? != 0 { SetupMode::Primary } else if word(3)? != 0 { SetupMode::Secondary } else { SetupMode::Other },
            owner: ControlOwner(word(0)?),
        }),
        Teardown | Synchronize | StartPolling | Resume | Suspend => {
            let usage = match opcode {
                Teardown => NativeCallbackUse::Teardown, Synchronize => NativeCallbackUse::Synchronize,
                StartPolling => NativeCallbackUse::StartPolling, Resume => NativeCallbackUse::Resume,
                Suspend => NativeCallbackUse::Suspend, _ => unreachable!(),
            };
            ControlPayload::Callback(callback(registry, if payload.is_empty() { 0 } else { word(0)? }, usage)?)
        }
        Response => {
            let kind = ResponseKind::try_from(word(0)?)?;
            let status = CompletionStatus(word(1)? as i32);
            let mut data = [0; 16];
            data.copy_from_slice(&payload[8..24]);
            let page = if matches!(kind, ResponseKind::Reply19 | ResponseKind::Reply21)
                && status == CompletionStatus::SUCCESS && u16::from_le_bytes([data[0], data[1]]) == 0 {
                Some(memory.calibration_page(NativeMemoryAddress(word(3)?))?)
            } else { None };
            ControlPayload::Response(ControlResponse { kind, status, data, page })
        }
        ModeComplete => ControlPayload::ModeComplete(match word(0)? {
            0xf => ModeCompletion::Resume, 0x10 => ModeCompletion::Suspend,
            _ => return Err(ControlError::InvalidPayload),
        }),
        InitializationFinished => ControlPayload::InitializationFinished(word(0)? != 0),
        Poll => ControlPayload::Poll,
        BeginDetection | Ready | Finish | Attached | Detached | TeardownComplete => ControlPayload::Empty,
    };
    Ok(ControlMessage { opcode, payload })
}
fn message_callback(message: ControlMessage) -> Option<CallbackToken> {
    match message.payload {
        ControlPayload::Register { completion, .. } => completion,
        ControlPayload::Setup(request) => request.completion,
        ControlPayload::Callback(callback) => callback,
        _ => None,
    }
}
impl QueueSender<'_> {
    pub fn ingress(&self, opcode: u16, payload: &[u8], registry: &impl NativeCallbackRegistry, memory: &(impl NativeMemory + ?Sized)) -> Result<(), ControlError> {
        let message = decode_message(opcode, payload, registry, memory)?;
        if let Err(error) = self.post(message) {
            if let Some(callback) = message_callback(message) { registry.release(callback); }
            return Err(error);
        }
        Ok(())
    }
}
