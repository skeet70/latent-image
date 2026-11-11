use core::cell::{Cell, RefCell};
use core::future::{Future, poll_fn};
use core::pin::Pin;
use core::task::{Context, Poll, Waker};
use crate::backend::BackendError;
use crate::configuration_storage::{ConfigurationFlash, ConfigurationStorage, UploadStatus};
use crate::lens_queue_runtime::{LensQueueRuntime, Publication, Status, packets::{CallbackId, QueueRequest}};
use crate::serial_program::{self, Port, SerialProgramState};
use crate::updater_peripheral::{PassLimit, PeripheralSnapshot, io::PeripheralIo, transaction::{Failure, PeripheralUpdater, PollStatus}};
use crate::upload_transaction::{self, Admission, UploadResult, UploadTarget};
use xm1_lens_transport::control_runtime::Task99Client;
use xm1_lens_transport::control_types::{CompletionStatus, ControlOwner, SetupMode};
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;

pub struct UploadPublications {
    expected: Cell<Option<CallbackId>>,
    publication: Cell<Option<Publication>>,
    waiter: RefCell<Option<Waker>>,
}
impl UploadPublications {
    pub const fn new() -> Self { Self { expected: Cell::new(None), publication: Cell::new(None), waiter: RefCell::new(None) } }
    pub fn publish(&self, publication: Publication) -> bool {
        let callback = match publication {
            Publication::Lifecycle { callback } | Publication::Command { callback, .. } | Publication::Write { callback, .. } | Publication::Read { callback, .. } | Publication::Observer { callback, .. } => callback,
        };
        if self.expected.get() != Some(callback) || self.publication.get().is_some() { return false; }
        self.publication.set(Some(publication));
        if let Some(waiter) = self.waiter.borrow_mut().take() { waiter.wake(); }
        true
    }
    pub(crate) fn expect(&self, callback: CallbackId) -> bool {
        if self.expected.get().is_some() { return false; }
        self.expected.set(Some(callback));
        true
    }
    pub(crate) fn take(&self) -> Option<Publication> {
        let publication = self.publication.take()?;
        self.expected.set(None);
        Some(publication)
    }
    pub(crate) fn clear(&self) { self.expected.set(None); self.publication.set(None); self.waiter.borrow_mut().take(); }
}

mod sealed { pub trait Clock {} }
pub trait Clock: sealed::Clock {
    fn tick(&self) -> u32;
    fn register(&self, waker: &Waker);
}
impl<C: Clock> sealed::Clock for &C {}
impl<C: Clock> Clock for &C {
    fn tick(&self) -> u32 { C::tick(self) }
    fn register(&self, waker: &Waker) { C::register(self, waker); }
}
pub struct HostClock { tick: Cell<u32>, waiter: RefCell<Option<Waker>> }
impl HostClock {
    pub const fn new(tick: u32) -> Self { Self { tick: Cell::new(tick), waiter: RefCell::new(None) } }
    pub fn advance(&self, ticks: u32) {
        self.tick.set(self.tick.get().wrapping_add(ticks));
        if let Some(waiter) = self.waiter.borrow_mut().take() { waiter.wake(); }
    }
}
impl sealed::Clock for HostClock {}
impl Clock for HostClock {
    fn tick(&self) -> u32 { self.tick.get() }
    fn register(&self, waker: &Waker) { *self.waiter.borrow_mut() = Some(waker.clone()); }
}
#[cfg(target_arch = "arm")]
pub struct NativeClock;
#[cfg(target_arch = "arm")]
impl sealed::Clock for NativeClock {}
#[cfg(target_arch = "arm")]
impl Clock for NativeClock {
    fn tick(&self) -> u32 { unsafe { core::ptr::read_volatile(0x400c0800 as *const u32) } }
    fn register(&self, waker: &Waker) { waker.wake_by_ref(); }
}

pub struct Delay<'a, C: Clock> { clock: &'a C, deadline: u32 }
impl<'a, C: Clock> Delay<'a, C> {
    pub fn new(clock: &'a C, ticks: u32) -> Self { Self { clock, deadline: clock.tick().wrapping_add(ticks) } }
}
impl<C: Clock> Future for Delay<'_, C> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.clock.tick().wrapping_sub(self.deadline) < 0x8000_0000 { Poll::Ready(()) }
        else { self.clock.register(cx.waker()); Poll::Pending }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UploadToken(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadPublication { Idle, Running(UploadToken), Finished { token: UploadToken, result: UploadResult }, Cancelled(UploadToken) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadOutcome { Finished(UploadResult), Cancelled }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadError { Busy, Exhausted, Image(BackendError) }

pub struct UploadControl {
    next: Cell<u64>,
    publication: Cell<UploadPublication>,
    cancelled: Cell<bool>,
    waiter: RefCell<Option<Waker>>,
}
impl UploadControl {
    pub const fn new() -> Self { Self { next: Cell::new(1), publication: Cell::new(UploadPublication::Idle), cancelled: Cell::new(false), waiter: RefCell::new(None) } }
    pub fn publication(&self) -> UploadPublication { self.publication.get() }
    pub fn cancel(&self, token: UploadToken) -> bool {
        if self.publication.get() != UploadPublication::Running(token) { return false; }
        self.cancelled.set(true);
        if let Some(waiter) = self.waiter.borrow_mut().take() { waiter.wake(); }
        true
    }
    fn begin(&self) -> Result<UploadToken, UploadError> {
        if matches!(self.publication.get(), UploadPublication::Running(_)) { return Err(UploadError::Busy); }
        let next = self.next.get();
        self.next.set(next.checked_add(1).ok_or(UploadError::Exhausted)?);
        let token = UploadToken(next);
        self.cancelled.set(false);
        self.publication.set(UploadPublication::Running(token));
        Ok(token)
    }
}

struct Lease<'a, 't, M: Mmio, P: Platform, I: InterruptController> {
    queue: &'a RefCell<LensQueueRuntime<M, P, I>>,
    lens: &'a Task99Client<'t>,
    control: &'a UploadControl,
    publications: &'a UploadPublications,
    token: UploadToken,
    peripheral: bool,
    finished: bool,
}
impl<M: Mmio, P: Platform, I: InterruptController> Drop for Lease<'_, '_, M, P, I> {
    fn drop(&mut self) {
        if self.peripheral && !self.finished {
            let _ = self.queue.borrow_mut().cancel();
            let _ = self.lens.finish();
        }
        if !self.finished { self.control.publication.set(UploadPublication::Cancelled(self.token)); }
        self.control.waiter.borrow_mut().take();
        self.publications.clear();
    }
}

pub struct UploadRuntime<'a, 't, M: Mmio, P: Platform, I: InterruptController, S: Port, F: ConfigurationFlash, C: Clock> {
    pub queue: &'a RefCell<LensQueueRuntime<M, P, I>>,
    pub lens: &'a Task99Client<'t>,
    pub publications: &'a UploadPublications,
    pub serial: S,
    pub storage: ConfigurationStorage<F>,
    pub clock: C,
    serial_state: SerialProgramState,
}
impl<'a, 't, M: Mmio, P: Platform, I: InterruptController, S: Port, F: ConfigurationFlash, C: Clock> UploadRuntime<'a, 't, M, P, I, S, F, C> {
    pub fn new(queue: &'a RefCell<LensQueueRuntime<M, P, I>>, lens: &'a Task99Client<'t>, publications: &'a UploadPublications, serial: S, storage: ConfigurationStorage<F>, clock: C) -> Self {
        Self { queue, lens, publications, serial, storage, clock, serial_state: SerialProgramState::new() }
    }
    pub async fn run<G: PeripheralIo>(&mut self, workspace: &mut [u8], total_size: u32, gpio: G, limit: PassLimit, owner: ControlOwner, control: &UploadControl) -> Result<UploadOutcome, UploadError> {
        let admission = upload_transaction::admit(workspace).map_err(UploadError::Image)?;
        let token = control.begin()?;
        let mut lease = Lease { queue: self.queue, lens: self.lens, control, publications: self.publications, token, peripheral: matches!(admission, Admission::Accepted(UploadTarget::SerialPeripheral | UploadTarget::PacketPeripheral)), finished: false };
        let outcome = {
            let operation = self.execute(admission, workspace, total_size, gpio, limit, owner);
            let mut operation = core::pin::pin!(operation);
            poll_fn(|cx| {
                *control.waiter.borrow_mut() = Some(cx.waker().clone());
                if control.cancelled.get() { return Poll::Ready(UploadOutcome::Cancelled); }
                operation.as_mut().poll(cx).map(UploadOutcome::Finished)
            }).await
        };
        match outcome {
            UploadOutcome::Finished(result) => {
                lease.finished = true;
                drop(lease);
                control.publication.set(UploadPublication::Finished { token, result });
            }
            UploadOutcome::Cancelled => drop(lease),
        }
        Ok(outcome)
    }
    async fn execute<G: PeripheralIo>(&mut self, admission: Admission, workspace: &mut [u8], total_size: u32, gpio: G, limit: PassLimit, owner: ControlOwner) -> UploadResult {
        let status = match admission {
            Admission::Rejected(status) => status,
            Admission::Accepted(UploadTarget::Configuration) => self.storage.upload_fallback(workspace, total_size).unwrap_or(UploadStatus::InvalidImage),
            Admission::Accepted(UploadTarget::SerialPeripheral) => self.serial_upload(workspace).await,
            Admission::Accepted(UploadTarget::PacketPeripheral) => self.packet_upload(workspace, gpio, limit, owner).await,
        };
        UploadResult { status }
    }
    async fn serial_upload(&mut self, image: &mut [u8]) -> UploadStatus {
        if self.queue.borrow_mut().cancel().is_err() { return UploadStatus::DriverFailure; }
        let mut power = serial_program::PowerLease::new(&mut self.serial);
        if serial_program::run(power.port(), &mut self.serial_state, image, &self.clock).await.is_err() { return UploadStatus::DriverFailure; }
        if self.queue.borrow_mut().submit(QueueRequest::Initialize { callback: CallbackId(0), observer: CallbackId(0) }, None).is_err() { return UploadStatus::DriverFailure; }
        Delay::new(&self.clock, 3000).await;
        let mut result = UploadStatus::Complete;
        for _ in 0..3 {
            result = queue_completion(self.queue, self.publications).await;
            if result != UploadStatus::Complete { break; }
        }
        Delay::new(&self.clock, 1000).await;
        result
    }
    async fn packet_upload<G: PeripheralIo>(&mut self, image: &[u8], gpio: G, limit: PassLimit, owner: ControlOwner) -> UploadStatus {
        match self.lens.setup(false, SetupMode::Primary, owner).await {
            Ok(CompletionStatus(0)) => {},
            Ok(CompletionStatus(status)) => return UploadStatus::PeripheralFailure(status),
            Err(_) => return UploadStatus::DriverFailure,
        }
        for _ in 0..500 {
            if self.lens.readiness() { break; }
            Delay::new(&self.clock, 10).await;
        }
        if self.lens.finish().is_err() { return UploadStatus::DriverFailure; }
        let mut snapshot = [0; 128];
        self.lens.snapshot_descriptor(&mut snapshot);
        let mut updater = match PeripheralUpdater::begin(self.queue, self.publications, gpio, image, PeripheralSnapshot::from_driver_bytes(snapshot), limit) {
            Ok(updater) => updater,
            Err(error) => return peripheral_status(error),
        };
        poll_fn(|cx| match updater.poll() {
            Ok(PollStatus::Complete) => Poll::Ready(UploadStatus::Complete),
            Ok(PollStatus::Cancelled) => Poll::Ready(UploadStatus::DriverFailure),
            Ok(PollStatus::Pending) => { cx.waker().wake_by_ref(); Poll::Pending },
            Err(error) => Poll::Ready(peripheral_status(error)),
        }).await
    }
}
async fn queue_completion<M: Mmio, P: Platform, I: InterruptController>(queue: &RefCell<LensQueueRuntime<M, P, I>>, publications: &UploadPublications) -> UploadStatus {
    let callback = match queue.borrow_mut().allocate_callback() { Ok(callback) => callback, Err(_) => return UploadStatus::DriverFailure };
    if !publications.expect(callback) { return UploadStatus::DriverFailure; }
    let mut bytes = [0; 48];
    bytes[..4].copy_from_slice(&callback.0.to_le_bytes());
    bytes[4..8].copy_from_slice(&0x3fc0u32.to_le_bytes());
    bytes[44] = 1;
    let request = match QueueRequest::parse(RequestId(0x7202), &bytes) { Ok(request) => request, Err(_) => return UploadStatus::DriverFailure };
    if queue.borrow_mut().submit(request, None).is_err() { return UploadStatus::DriverFailure; }
    poll_fn(|cx| {
        *publications.waiter.borrow_mut() = Some(cx.waker().clone());
        match publications.take() {
            Some(Publication::Command { callback: received, operation, status, .. }) if received == callback && operation.request_id() == RequestId(0x7202) => Poll::Ready(if status == Status::Complete { UploadStatus::Complete } else { UploadStatus::PeripheralFailure(status.driver_word() as i32) }),
            Some(_) => Poll::Ready(UploadStatus::CompletionFailure),
            None => Poll::Pending,
        }
    }).await
}

fn peripheral_status<E>(error: Failure<E>) -> UploadStatus {
    match error {
        Failure::Image(_) => UploadStatus::InvalidImage,
        Failure::Status(status) => UploadStatus::PeripheralFailure(status.driver_word() as i32),
        Failure::Rejected => UploadStatus::PeripheralFailure(-255),
        Failure::Timeout => UploadStatus::CompletionFailure,
        _ => UploadStatus::DriverFailure,
    }
}

#[cfg(test)]
#[path = "upload_runtime_tests.rs"]
mod tests;
