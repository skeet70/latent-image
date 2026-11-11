//! Task 99 native root: exclusive reservations, queue ingress and the owned dispatch loop.
#![cfg(target_arch = "arm")]

use core::cell::UnsafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use crate::control_actor::{ControlActor, ControlConstructor};
use crate::control_callbacks::CallbackRegistry;
use crate::control_device::ControlQueue;
use crate::control_native::*;
use crate::control_publication::ControlOutbox;
use crate::control_runtime::{dispatch_task99, CompletionMailbox, ControlPublications};
use crate::control_types::*;
use crate::key_ui::{NotificationState, PublicationServices, Publisher};

pub struct Task99Reservations { queue: KernelSlot, task: KernelSlot, timers: [KernelSlot; 5] }
impl Task99Reservations {
    /// Queues, tasks and cyclic timers are separate kernel tables, so only the timers must be distinct.
    /// Power posts the 0x73xx opcodes to kernel queue 99.
    pub fn new(queue: KernelSlot, task: KernelSlot, timers: [KernelSlot; 5]) -> Result<Self, ControlError> {
        if queue.raw() != 99 { return Err(ControlError::Constructor); }
        for (index, slot) in timers.iter().enumerate() {
            if timers[..index].contains(slot) { return Err(ControlError::Constructor); }
        }
        Ok(Self { queue, task, timers })
    }
    pub const fn queue(&self) -> KernelSlot { self.queue }
    pub const fn task(&self) -> KernelSlot { self.task }
    pub const fn timers(&self) -> [KernelSlot; 5] { self.timers }
}

pub struct Task99Parts<E, P, R, K, Q> {
    pub reservations: Task99Reservations,
    pub queue: &'static Task99Queue,
    pub entry: NativeDomain,
    pub argument: u32,
    pub receiver: NativeReceiver<'static>,
    pub actor: ControlActor<'static>,
    pub publications: &'static ControlPublications,
    pub outbox: ControlOutbox,
    pub io: CpuControlIo<NativeServices<E>, P, R, K>,
    pub queue98: Q,
}

pub struct Task99Root<E: NativeEffects, P: ControlPower, R: ControlProfiles, K: KeyPublicationSink, Q: ControlQueue> {
    state: UnsafeCell<MaybeUninit<Task99Parts<E, P, R, K, Q>>>,
    initialized: AtomicBool,
    running: AtomicBool,
}
// `initialize` writes `state` before task activation; `run` is the only reader afterwards.
unsafe impl<E: NativeEffects, P: ControlPower, R: ControlProfiles, K: KeyPublicationSink, Q: ControlQueue> Sync for Task99Root<E, P, R, K, Q> {}
impl<E: NativeEffects, P: ControlPower, R: ControlProfiles, K: KeyPublicationSink, Q: ControlQueue> Task99Root<E, P, R, K, Q> {
    pub const fn new() -> Self { Self { state: UnsafeCell::new(MaybeUninit::uninit()), initialized: AtomicBool::new(false), running: AtomicBool::new(false) } }
    /// # Safety
    /// The reservations name slots admitted by the selected RootPackage, `entry` is the domain that activates the task, and `stack` is reserved for it.
    pub unsafe fn initialize(&'static self, parts: Task99Parts<E, P, R, K, Q>, stack: &'static mut TaskStack) -> Result<(), ControlError> {
        if self.initialized.load(Ordering::Acquire) { return Err(ControlError::Constructor); }
        if ENTRY.load().is_none() { return Err(ControlError::Constructor); }
        if parts.queue.slot() != parts.reservations.queue() { return Err(ControlError::Constructor); }
        for (timer, slot) in parts.io.services.timers.iter().zip(parts.reservations.timers()) {
            if timer.slot() != slot { return Err(ControlError::Constructor); }
        }
        let mut constructor = unsafe { NativeConstructor::new(parts.queue, parts.reservations.task(), parts.entry, stack, parts.argument) };
        constructor.create_queue(99, 48, 32)?;
        unsafe { (*self.state.get()).write(parts) };
        self.initialized.store(true, Ordering::Release);
        if let Err(error) = constructor.create_task(99, 4, 3072) {
            self.initialized.store(false, Ordering::Release);
            return Err(error);
        }
        Ok(())
    }
    pub fn run(&'static self) -> ! {
        if self.running.swap(true, Ordering::AcqRel) { loop { core::hint::spin_loop(); } }
        if !self.initialized.load(Ordering::Acquire) { loop { core::hint::spin_loop(); } }
        let state = unsafe { (*self.state.get()).assume_init_mut() };
        state.publications.publish(&state.actor);
        loop {
            let Ok(message) = state.receiver.receive_now() else { core::hint::spin_loop(); continue; };
            let _ = dispatch_task99(&mut state.actor, message, &mut state.io, &mut state.queue98, state.publications, &mut state.outbox);
        }
    }
}

struct EntrySlot(AtomicUsize);
impl EntrySlot {
    const fn new() -> Self { Self(AtomicUsize::new(0)) }
    fn install(&self, entry: fn() -> !) -> bool { self.0.compare_exchange(0, entry as usize, Ordering::AcqRel, Ordering::Acquire).is_ok() }
    fn load(&self) -> Option<fn() -> !> {
        let value = self.0.load(Ordering::Acquire);
        if value == 0 { None } else { Some(unsafe { core::mem::transmute::<usize, fn() -> !>(value) }) }
    }
}
static ENTRY: EntrySlot = EntrySlot::new();

pub fn install_task99_entry(entry: fn() -> !) -> Result<(), ControlError> {
    if ENTRY.install(entry) { Ok(()) } else { Err(ControlError::Busy) }
}

/// Entry veneer target from `control-entry.S`.
#[no_mangle]
pub extern "C" fn xm1_lens_task99_run() -> ! {
    match ENTRY.load() {
        Some(entry) => entry(),
        None => loop { core::hint::spin_loop() },
    }
}

pub struct Task99Ingress {
    queue: &'static Task99Queue,
    entry: NativeDomain,
    registry: &'static CallbackRegistry,
    memory: &'static dyn NativeMemory,
}
impl Task99Ingress {
    pub const fn new(queue: &'static Task99Queue, entry: NativeDomain, registry: &'static CallbackRegistry, memory: &'static dyn NativeMemory) -> Self {
        Self { queue, entry, registry, memory }
    }
    pub fn try_post(&self, queue: u8, operation: u16, payload: &[u8]) -> Option<Result<(), ControlError>> {
        if u16::from(queue) != self.queue.slot().raw() { return None; }
        Some(self.queue.sender(self.entry).ingress(operation, payload, self.registry, self.memory))
    }
}

pub trait LensDiagnostics { fn emit(&mut self, category: u32, event: u32); }
pub trait DetectionControl { fn set_callback(&mut self, edge: DetectionEdge, enabled: bool); }
pub trait LensGpio { fn reset(&mut self); }
pub trait CompletionFallback { fn complete(&mut self, token: CallbackToken, status: CompletionStatus, data: &[u8]); }

pub struct LensEffects<U, D, H, G, F> {
    pub registry: &'static CallbackRegistry,
    pub mailbox: &'static CompletionMailbox,
    pub services: U,
    pub state: NotificationState,
    pub diagnostic: D,
    pub detection: H,
    pub gpio: G,
    pub fallback: F,
}
impl<U: PublicationServices, D: LensDiagnostics, H: DetectionControl, G: LensGpio, F: CompletionFallback> NativeEffects for LensEffects<U, D, H, G, F> {
    fn complete(&mut self, callback: CallbackToken, status: CompletionStatus, data: &[u8]) {
        if let Some((address, _)) = self.registry.take(callback) {
            unsafe { address.invoke(status) }
        } else if self.mailbox.owns(callback) {
            self.mailbox.complete(callback, status, data);
        } else {
            self.fallback.complete(callback, status, data);
        }
    }
    fn notify(&mut self, target: WaitTarget) {
        let _ = unsafe { xm1_power_set_task_flags(u32::from(target.task), target.bits) };
    }
    fn key_release(&mut self, event: KeyEvent) {
        let mut publisher = Publisher { services: &mut self.services, state: &mut self.state };
        publisher.release(event, 0);
    }
    fn diagnostic(&mut self, category: u32, event: u32) { self.diagnostic.emit(category, event) }
    fn set_detection_callback(&mut self, edge: DetectionEdge, enabled: bool) { self.detection.set_callback(edge, enabled) }
    fn reset_gpio(&mut self) { self.gpio.reset() }
}

extern "C" { fn xm1_power_set_task_flags(task: u32, bits: u32) -> i32; }
