//! Pointer arguments remain valid for their operation; callback storage and context remain valid until unregister returns successfully.
//! Registration runs on the core owning the selected IRQ-controller domain, after every other user releases serial 2, DMA 2/3, timer 11 and the scratch windows.

use crate::{frame::*, wire::{TimerTicks, TimerToken, TransferToken, MAX_TRANSFER_BYTES}, wire_native::{backend_exclusive, enter_critical, install_irq_handlers, leave_critical, uninstall_irq_handlers, NativeBackend, SavedInterrupts}, CallbackDelay, CompletionSink, CompletionStatus, Error, FailureMode, GenerationCheckpoint, PageAcceptance, RequestToken, TimingParameters, TimingProfile, TransferOption, Transport};
use core::cell::UnsafeCell;
use core::ffi::c_void;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Status { Complete = 0, NotRegistered = -1, Busy = -2, InvalidInput = -3, BackendFailure = -4, InvalidFrame = -5, StaleCompletion = -6, Exhausted = -7 }

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Callbacks {
    pub context: *mut c_void,
    pub command: Option<unsafe extern "C" fn(*mut c_void, u32, u32) -> u32>,
    pub written: Option<unsafe extern "C" fn(*mut c_void, u32, u16, u32)>,
    pub read: Option<unsafe extern "C" fn(*mut c_void, u32, u16, *const u8, u32)>,
    pub terminated: Option<unsafe extern "C" fn(*mut c_void, u32, u32)>,
    pub failed: Option<unsafe extern "C" fn(*mut c_void)>,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct Timing {
    pub command_ms: u32,
    pub normal_ms: u32,
    pub deferred_ms: u32,
    pub accepted_page_ms: u32,
    pub rejected_page_ms: u32,
    pub recovery_ms: u32,
    pub recovery_limit: u32,
    pub busy_limit: u32,
    pub diagnostic_blink: u32,
}
impl Timing {
    fn parse(self) -> Option<TimingParameters> {
        Some(TimingParameters {
            command: TimerTicks::new(self.command_ms)?, normal: TimerTicks::new(self.normal_ms)?,
            deferred: TimerTicks::new(self.deferred_ms)?, accepted_page: TimerTicks::new(self.accepted_page_ms)?,
            rejected_page: TimerTicks::new(self.rejected_page_ms)?, recovery: TimerTicks::new(self.recovery_ms)?,
            recovery_limit: RetryLimit::new(self.recovery_limit), busy_limit: RetryLimit::new(self.busy_limit),
            failure_mode: match self.diagnostic_blink { 0 => FailureMode::Notify, 1 => FailureMode::DiagnosticBlink, _ => return None },
        })
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct TimingProfileRecord {
    pub command_ms: u32,
    pub normal_ms: u32,
    pub deferred_ms: u32,
    pub accepted_page_ms: u32,
    pub rejected_page_ms: u32,
}
impl TimingProfileRecord {
    fn parse(self) -> Option<TimingProfile> {
        Some(TimingProfile {
            command: TimerTicks::new(self.command_ms)?, normal: TimerTicks::new(self.normal_ms)?,
            deferred: TimerTicks::new(self.deferred_ms)?, accepted_page: TimerTicks::new(self.accepted_page_ms)?,
            rejected_page: TimerTicks::new(self.rejected_page_ms)?,
        })
    }
}

struct NativeCompletions {
    context: *mut c_void,
    command: unsafe extern "C" fn(*mut c_void, u32, u32) -> u32,
    written: unsafe extern "C" fn(*mut c_void, u32, u16, u32),
    read: unsafe extern "C" fn(*mut c_void, u32, u16, *const u8, u32),
    terminated: unsafe extern "C" fn(*mut c_void, u32, u32),
    failed: unsafe extern "C" fn(*mut c_void),
}
impl NativeCompletions {
    fn parse(callbacks: Callbacks) -> Option<Self> {
        Some(Self { context: callbacks.context, command: callbacks.command?, written: callbacks.written?, read: callbacks.read?, terminated: callbacks.terminated?, failed: callbacks.failed? })
    }
}
impl CompletionSink for NativeCompletions {
    fn command_completed(&mut self, token: RequestToken, response: CommandWord) -> CallbackDelay {
        if unsafe { (self.command)(self.context, token.get(), response.raw()) } == 0 { CallbackDelay::Normal } else { CallbackDelay::Deferred }
    }
    fn page_written(&mut self, token: RequestToken, response: PageWriteCompletion) {
        unsafe { (self.written)(self.context, token.get(), response.page.get(), u32::from(response.marker_valid)); }
    }
    fn page_read(&mut self, token: RequestToken, page: PageId, bytes: &[u8]) {
        unsafe { (self.read)(self.context, token.get(), page.get(), bytes.as_ptr(), bytes.len() as u32); }
    }
    fn terminated(&mut self, token: RequestToken, status: CompletionStatus) {
        let status = match status { CompletionStatus::Complete => 0, CompletionStatus::Failed => 1, CompletionStatus::Cancelled => 2, CompletionStatus::Reset => 3, CompletionStatus::Teardown => 4 };
        unsafe { (self.terminated)(self.context, token.get(), status); }
    }
    fn transport_failed(&mut self) { unsafe { (self.failed)(self.context); } }
}

type NativeTransport = Transport<NativeBackend, NativeCompletions>;
struct ServiceSlot(UnsafeCell<MaybeUninit<NativeTransport>>);
// Every access to the service storage holds Access.
unsafe impl Sync for ServiceSlot {}
static SERVICE: ServiceSlot = ServiceSlot(UnsafeCell::new(MaybeUninit::uninit()));
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static GENERATION: AtomicU32 = AtomicU32::new(0);
struct Access(SavedInterrupts);
impl Access {
    fn acquire() -> Result<Self, Status> {
        let saved = enter_critical();
        if BUSY.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            leave_critical(saved);
            return Err(Status::Busy);
        }
        Ok(Self(saved))
    }
}
impl Drop for Access {
    fn drop(&mut self) { BUSY.store(false, Ordering::Release); leave_critical(self.0); }
}
fn with_service(operation: impl FnOnce(&mut NativeTransport) -> Status) -> Status {
    let _access = match Access::acquire() { Ok(access) => access, Err(status) => return status };
    if !INITIALIZED.load(Ordering::Acquire) { return Status::NotRegistered; }
    let service = unsafe { (&mut *SERVICE.0.get()).assume_init_mut() };
    let result = operation(service);
    GENERATION.store(service.generation_checkpoint().get(), Ordering::Release);
    result
}
fn status<E>(result: Result<(), Error<E>>) -> Status {
    match result {
        Ok(()) => Status::Complete,
        Err(Error::Backend(_)) => Status::BackendFailure,
        Err(Error::Frame(_)) => Status::InvalidFrame,
        Err(Error::Queue(_) | Error::Busy | Error::Unexpected { .. }) => Status::Busy,
        Err(Error::Stopped) => Status::NotRegistered,
        Err(Error::LateCompletion) => Status::StaleCompletion,
        Err(Error::TokenExhausted) => Status::Exhausted,
    }
}
fn valid<T>(pointer: *const T) -> bool { !pointer.is_null() && (pointer as usize) % core::mem::align_of::<T>() == 0 }
fn request<E>(result: Result<RequestToken, Error<E>>, output: *mut u32) -> Status {
    match result { Ok(token) => { unsafe { output.write(token.get()); } Status::Complete }, Err(error) => status(Err(error)) }
}

#[no_mangle]
pub unsafe extern "C" fn xm1_lens_initialize(callbacks: *const Callbacks, timing: *const Timing) -> Status {
    if !valid(callbacks) || !valid(timing) { return Status::InvalidInput; }
    let callbacks = match NativeCompletions::parse(callbacks.read()) { Some(callbacks) => callbacks, None => return Status::InvalidInput };
    let timing = match timing.read().parse() { Some(timing) => timing, None => return Status::InvalidInput };
    let _access = match Access::acquire() { Ok(access) => access, Err(status) => return status };
    if INITIALIZED.load(Ordering::Acquire) { return Status::Busy; }
    if unsafe { install_irq_handlers(xm1_lens_abi_wire_irq, xm1_lens_abi_timer_irq) }.is_err() { return Status::Busy; }
    let generation = GenerationCheckpoint::from_abi(GENERATION.load(Ordering::Acquire));
    let service = (&mut *SERVICE.0.get()).write(Transport::resume(backend_exclusive(), callbacks, timing, generation));
    let result = status(service.initialize());
    if result != Status::Complete { uninstall_irq_handlers(xm1_lens_abi_wire_irq, xm1_lens_abi_timer_irq); }
    INITIALIZED.store(result == Status::Complete, Ordering::Release);
    result
}

#[no_mangle]
pub unsafe extern "C" fn xm1_lens_command(command: u32, output: *mut u32) -> Status {
    if !valid(output) { return Status::InvalidInput; }
    with_service(|service| request(service.enqueue_command(CommandWord::new(command)), output))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_lens_write(page: u16, bytes: *const u8, length: u32, checksum: u32, output: *mut u32) -> Status {
    if !valid(output) || (length != 0 && bytes.is_null()) || PayloadLength::new(length as usize).is_err() { return Status::InvalidInput; }
    let checksum = match checksum { 0 => ChecksumMode::Disabled, 1 => ChecksumMode::Enabled, _ => return Status::InvalidInput };
    let bytes = if length == 0 { &[] } else { core::slice::from_raw_parts(bytes, length as usize) };
    with_service(|service| request(service.write_page(PageId::new(page), bytes, checksum), output))
}

#[no_mangle]
pub unsafe extern "C" fn xm1_lens_read(page: u16, length: u32, extra_byte: u32, marker: u32, output: *mut u32) -> Status {
    if !valid(output) { return Status::InvalidInput; }
    let length = match PayloadLength::new(length as usize) { Ok(length) => length, Err(_) => return Status::InvalidInput };
    let option = match extra_byte { 0 => TransferOption::Plain, 1 => TransferOption::ExtraByte, _ => return Status::InvalidInput };
    let marker = match marker { 0 => ReadMarker::Disabled, 1 => ReadMarker::Enabled, _ => return Status::InvalidInput };
    with_service(|service| request(service.read_page(PageId::new(page), length, option, marker), output))
}

#[no_mangle]
pub extern "C" fn xm1_lens_accept(accepted: u32) -> Status {
    let acceptance = match accepted { 0 => PageAcceptance::Rejected, 1 => PageAcceptance::Accepted, _ => return Status::InvalidInput };
    with_service(|service| status(service.accept_page(acceptance)))
}
#[no_mangle]
pub extern "C" fn xm1_lens_cancel() -> Status { with_service(|service| status(service.cancel())) }
#[no_mangle]
pub extern "C" fn xm1_lens_reset() -> Status { with_service(|service| status(service.reset())) }
#[no_mangle]
pub unsafe extern "C" fn xm1_lens_replace_timing(timing: *const TimingProfileRecord) -> Status {
    if !valid(timing) { return Status::InvalidInput; }
    let timing = match timing.read().parse() { Some(timing) => timing, None => return Status::InvalidInput };
    with_service(|service| { service.replace_timing(timing); Status::Complete })
}
#[no_mangle]
pub unsafe extern "C" fn xm1_lens_set_timing(timing: *const TimingProfileRecord) -> Status {
    if !valid(timing) { return Status::InvalidInput; }
    let timing = match timing.read().parse() { Some(timing) => timing, None => return Status::InvalidInput };
    with_service(|service| { service.set_timing(timing); Status::Complete })
}
#[no_mangle]
pub extern "C" fn xm1_lens_unregister() -> Status {
    with_service(|service| {
        let result = status(service.stop());
        if result == Status::Complete {
            INITIALIZED.store(false, Ordering::Release);
            uninstall_irq_handlers(xm1_lens_abi_wire_irq, xm1_lens_abi_timer_irq);
        }
        result
    })
}

/// The bounded ABI transport completes through these handlers once `xm1_lens_initialize` installs them.
pub unsafe extern "C" fn xm1_lens_abi_wire_irq(raw_token: u32) {
    let Some(token) = TransferToken::new(raw_token) else { return; };
    with_service(|service| {
        let mut received = [0_u8; MAX_TRANSFER_BYTES];
        match service.backend_mut().complete_wire(token, &mut received) {
            Ok(length) => {
                let result = status(service.receive(token, &received[..length]));
                if result != Status::Complete && result != Status::StaleCompletion { let _ = service.fail(); }
                result
            },
            Err(crate::wire::Error::StaleToken | crate::wire::Error::NotRegistered) => Status::StaleCompletion,
            Err(_) => { let _ = service.fail(); Status::BackendFailure },
        }
    });
}
pub unsafe extern "C" fn xm1_lens_abi_timer_irq(raw_token: u32) {
    let Some(token) = TimerToken::new(raw_token) else { return; };
    with_service(|service| {
        match service.backend_mut().complete_timer(token) {
            Ok(()) => {
                let result = status(service.timer_expired(token));
                if result != Status::Complete && result != Status::StaleCompletion { let _ = service.fail(); }
                result
            },
            Err(crate::wire::Error::StaleToken | crate::wire::Error::NotRegistered) => Status::StaleCompletion,
            Err(_) => { let _ = service.fail(); Status::BackendFailure },
        }
    });
}
