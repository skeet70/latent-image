use super::{read, write, Error, OsalStatus, Timeout, WaitMode, NAND};

#[cfg(target_arch = "arm")]
use core::num::NonZeroU16;
#[cfg(target_arch = "arm")]
use core::sync::atomic::{AtomicU32, Ordering};

#[cfg(target_arch = "arm")]
const NO_WAITER: u32 = u32::MAX;
#[cfg(target_arch = "arm")]
static WAITER: AtomicU32 = AtomicU32::new(NO_WAITER);

#[cfg(target_arch = "arm")]
#[derive(Clone, Copy)]
struct TaskNumber(NonZeroU16);

#[cfg(target_arch = "arm")]
impl TaskNumber {
    fn parse(raw: u32) -> Result<Self, Error> {
        if raw == NO_WAITER { return Err(Error::TaskContext); }
        NonZeroU16::new(raw as u16).map(Self).ok_or(Error::TaskContext)
    }
    fn word(self) -> u32 { u32::from(self.0.get()) }
}

#[cfg(target_arch = "arm")]
pub(super) struct InterruptHandle(u32);

pub(super) enum PendingCommand {
    Poll,
    #[cfg(target_arch = "arm")]
    Interrupt(InterruptHandle),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventOperation { ClearFlags, AttachInterrupt, Wait, ReleaseInterrupt }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventFailure {
    pub operation: EventOperation,
    pub status: OsalStatus,
}

#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_current_task() -> u32;
    fn xm1_power_clear_current_flags() -> i32;
    fn xm1_power_wait_current_all(bits: u32, observed: *mut u32, timeout: i32) -> i32;
    fn xm1_power_set_task_flags(task: u32, bits: u32) -> i32;
    fn xm1_power_owned_callback_register(word: u32, shape: u32) -> i32;
    fn xm1_power_timer_irq_attach(source: u32, handler: u32, output: *mut u32) -> i32;
    fn xm1_power_timer_release_handle(handle: u32) -> i32;
}

/// `OwnedShape::Completion` in the Power Services leaf: `extern "C" fn()`.
#[cfg(target_arch = "arm")]
const COMPLETION_SHAPE: u32 = 0;

#[cfg(target_arch = "arm")]
fn event_status(operation: EventOperation, status: i32) -> Result<(), Error> {
    if status == 0 { Ok(()) } else { Err(Error::Event(EventFailure { operation, status: OsalStatus(status) })) }
}

/// The Power OSAL task and timer services exist only in the target image.
pub(super) unsafe fn prepare(mode: WaitMode) -> Result<PendingCommand, Error> {
    match mode {
        WaitMode::Poll => Ok(PendingCommand::Poll),
        #[cfg(target_arch = "arm")]
        WaitMode::Interrupt => unsafe { prepare_interrupt() },
        #[cfg(not(target_arch = "arm"))]
        WaitMode::Interrupt => Err(Error::TaskContext),
    }
}

#[cfg(target_arch = "arm")]
unsafe fn prepare_interrupt() -> Result<PendingCommand, Error> {
    let task = TaskNumber::parse(unsafe { xm1_power_current_task() })?;
    WAITER.store(task.word(), Ordering::Release);
    if let Err(error) = event_status(EventOperation::ClearFlags, unsafe { xm1_power_clear_current_flags() }) {
        WAITER.store(NO_WAITER, Ordering::Release);
        return Err(error);
    }
    let mut handle = 0;
    let handler = xm1_app_nand_ready_irq as *const () as u32;
    if let Err(error) = event_status(EventOperation::AttachInterrupt, unsafe { xm1_power_owned_callback_register(handler, COMPLETION_SHAPE) }) {
        WAITER.store(NO_WAITER, Ordering::Release);
        return Err(error);
    }
    if let Err(error) = event_status(EventOperation::AttachInterrupt, unsafe { xm1_power_timer_irq_attach(0x4a, handler, &mut handle) }) {
        WAITER.store(NO_WAITER, Ordering::Release);
        return Err(error);
    }
    unsafe { write(NAND + 0x54, 0x80009); }
    Ok(PendingCommand::Interrupt(InterruptHandle(handle)))
}

pub(super) unsafe fn wait(pending: PendingCommand, timeout: Timeout) -> Result<(), Error> {
    match pending {
        PendingCommand::Poll => {
            let mut polls = timeout.polls();
            loop {
                if unsafe { read(NAND + 4) } & 4 == 0 { return Ok(()); }
                polls -= 1;
                if polls == 0 { return Err(Error::Timeout); }
            }
        },
        #[cfg(target_arch = "arm")]
        PendingCommand::Interrupt(handle) => {
            let mut observed = 0;
            let status = unsafe { xm1_power_wait_current_all(0x100, &mut observed, timeout.0.wrapping_add(10) as i32) };
            unsafe { write(NAND + 0x54, 0x80001); }
            let released = event_status(EventOperation::ReleaseInterrupt, unsafe { xm1_power_timer_release_handle(handle.0) });
            WAITER.store(NO_WAITER, Ordering::Release);
            released?;
            if status == -50 { Err(Error::Timeout) } else { event_status(EventOperation::Wait, status) }
        }
    }
}

/// # Safety
/// The owned interrupt dispatcher invokes this handler for the registered NAND source 0x4a in interrupt context.
#[cfg(target_arch = "arm")]
#[no_mangle]
pub unsafe extern "C" fn xm1_app_nand_ready_irq() {
    let task = WAITER.swap(NO_WAITER, Ordering::AcqRel);
    if task != NO_WAITER { let _ = unsafe { xm1_power_set_task_flags(task, 0x100) }; }
}
