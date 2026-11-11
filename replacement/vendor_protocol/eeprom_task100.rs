//! Task 100 queue-98 completion adapter for the canonical power storage lifecycle.
//! X-M1 1.50 source pin: 55436fc85c0560dd252b0410af6f2427fe198cd3.

use core::cell::{Ref, RefCell, RefMut};
use core::future::poll_fn;
use core::task::{Poll, Waker};
use core::ops::{Deref, DerefMut};
use crate::backend::{ConfigImages, ConfigSpan};
use crate::lens_queue_runtime::{LensQueueRuntime, PageDisposition, Publication, Status};
use crate::lens_queue_runtime::packets::{CallbackId, QueueRequest};
use crate::lens_queue_runtime::replies::ReplyData;
use crate::eeprom_detection::{DetectionIo, DetectionProvider};
use crate::eeprom_profile_medium::ProfileMedium;
use crate::startup_projection::StartupProjection;
use xm1_power_services::storage_shutdown_medium::NandSoftware;
use xm1_power_services::storage_shutdown_defaults::DefaultStorageIo;
use xm1_lens_transport::wire::{InterruptController, Mmio, Platform};
use xm1_power_services::shutdown::RequestId;
use xm1_power_services::storage_shutdown_actor::MediumStatus;
use xm1_power_services::storage_shutdown_lifecycle::{LensCompletion, LensPageOperation};
use xm1_power_services::storage_shutdown_pages::{self, ConfigurationAddress, LensPageSoftware, PageBoundary, PageNumber};

#[derive(Debug, Eq, PartialEq)]
pub struct CallbackRange { next: u32, last: u32 }
impl CallbackRange {
    pub const fn new(first: CallbackId, last: CallbackId) -> Option<Self> {
        if first.0 == 0 || first.0 > last.0 { return None; }
        Some(Self { next: first.0, last: last.0 })
    }
    fn take(&mut self) -> Option<CallbackId> {
        if self.next == 0 || self.next > self.last { return None; }
        let id = CallbackId(self.next);
        self.next = self.next.checked_add(1).unwrap_or(0);
        Some(id)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdmissionError { Busy, TokenExhausted, Transport, Configuration, Timeout }
#[derive(Clone, Copy)]
enum Reply {
    Command { status: Status, data: ReplyData },
    Read { status: Status, page: PageNumber },
    Write { status: Status, page: PageNumber },
    Signal(MediumStatus),
}
struct Pending { callback: CallbackId, completion: LensCompletion, operation: Option<RequestId>, reply: Option<Reply>, started: u32, waiting: bool }
struct DelayState { remaining: u32, waker: Option<Waker> }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DispatchTicks(pub u32);
struct EventState {
    callbacks: CallbackRange,
    pending: Option<Pending>,
    dma: Option<Pending>,
    failure: Option<AdmissionError>,
    software_failure: Option<AdmissionError>,
    received: [u8; 0x800],
    received_bytes: usize,
    waker: Option<Waker>,
    ticks: u32,
    delay: Option<DelayState>,
}
struct Critical {
    #[cfg(target_arch = "arm")]
    saved: u32,
}
#[cfg(target_arch = "arm")]
extern "C" {
    fn xm1_power_interrupt_lock() -> u32;
    fn xm1_power_interrupt_unlock(saved: u32);
}
impl Critical {
    fn enter() -> Self {
        Self {
            #[cfg(target_arch = "arm")]
            saved: unsafe { xm1_power_interrupt_lock() },
        }
    }
}
impl Drop for Critical {
    fn drop(&mut self) {
        #[cfg(target_arch = "arm")]
        unsafe { xm1_power_interrupt_unlock(self.saved) };
    }
}
struct EventCell(RefCell<EventState>);
struct EventRef<'a> { value: Ref<'a, EventState>, _critical: Critical }
struct EventRefMut<'a> { value: RefMut<'a, EventState>, _critical: Critical }
impl Deref for EventRef<'_> { type Target = EventState; fn deref(&self) -> &EventState { &self.value } }
impl Deref for EventRefMut<'_> { type Target = EventState; fn deref(&self) -> &EventState { &self.value } }
impl DerefMut for EventRefMut<'_> { fn deref_mut(&mut self) -> &mut EventState { &mut self.value } }
impl EventCell {
    fn borrow(&self) -> EventRef<'_> {
        let critical = Critical::enter();
        EventRef { value: self.0.borrow(), _critical: critical }
    }
    fn borrow_mut(&self) -> EventRefMut<'_> {
        let critical = Critical::enter();
        EventRefMut { value: self.0.borrow_mut(), _critical: critical }
    }
}
pub struct EepromEvents { state: EventCell }
impl EepromEvents {
    pub const fn new(callbacks: CallbackRange) -> Self {
        Self { state: EventCell(RefCell::new(EventState { callbacks, pending: None, dma: None, failure: None, software_failure: None, received: [0; 0x800], received_bytes: 0, waker: None, ticks: 0, delay: None })) }
    }
    pub(crate) fn arm(&self, completion: LensCompletion) -> Result<CallbackId, AdmissionError> {
        let mut state = self.state.borrow_mut();
        let busy = if completion == LensCompletion::Dma { state.dma.is_some() } else { state.pending.is_some() };
        if busy { return Err(AdmissionError::Busy); }
        let callback = state.callbacks.take().ok_or(AdmissionError::TokenExhausted)?;
        let pending = Pending { callback, completion, operation: None, reply: None, started: state.ticks, waiting: false };
        if completion == LensCompletion::Dma { state.dma = Some(pending); } else { state.pending = Some(pending); }
        Ok(callback)
    }
    fn arm_command(&self, operation: RequestId, completion: LensCompletion) -> Result<CallbackId, AdmissionError> {
        let callback = self.arm(completion)?;
        self.state.borrow_mut().pending.as_mut().unwrap().operation = Some(operation);
        Ok(callback)
    }
    pub(crate) fn cancel_dma(&self) {
        let mut state = self.state.borrow_mut();
        state.dma = None;
    }
    pub fn software_failure(&self) -> Option<AdmissionError> { self.state.borrow().software_failure }
    pub(crate) fn fail_software(&self, error: AdmissionError) {
        self.state.borrow_mut().software_failure = Some(error);
        self.fail(error);
    }
    pub(crate) fn dma_callback(&self) -> Option<CallbackId> { self.state.borrow().dma.as_ref().map(|pending| pending.callback) }
    pub fn begin_request(&self) -> Result<(), AdmissionError> {
        let mut state = self.state.borrow_mut();
        if state.pending.is_some() || state.dma.is_some() || state.delay.is_some() { return Err(AdmissionError::Busy); }
        state.failure = None;
        state.software_failure = None;
        state.received_bytes = 0;
        Ok(())
    }
    pub fn advance_ticks(&self, ticks: DispatchTicks) {
        let (waiter, delay_waiter) = {
            let mut state = self.state.borrow_mut();
            state.ticks = state.ticks.wrapping_add(ticks.0);
            let expired = [&state.pending, &state.dma].into_iter().any(|slot| slot.as_ref().is_some_and(|pending| pending.waiting && pending.reply.is_none() && state.ticks.wrapping_sub(pending.started) >= 5000));
            let waiter = if expired { state.failure = Some(AdmissionError::Timeout); state.waker.take() } else { None };
            let delay_waiter = if let Some(delay) = state.delay.as_mut() {
                delay.remaining = delay.remaining.saturating_sub(ticks.0);
                if delay.remaining == 0 { delay.waker.take() } else { None }
            } else { None };
            (waiter, delay_waiter)
        };
        if let Some(waker) = waiter { waker.wake(); }
        if let Some(waker) = delay_waiter { waker.wake(); }
    }
    pub(crate) async fn delay(&self, ticks: u32) {
        {
            let mut state = self.state.borrow_mut();
            if state.delay.is_some() { state.failure = Some(AdmissionError::Busy); return; }
            state.delay = Some(DelayState { remaining: ticks, waker: None });
        }
        let _cancel = CancelDelay(self);
        poll_fn(|cx| {
            let mut state = self.state.borrow_mut();
            if state.failure.is_some() { return Poll::Ready(()); }
            let Some(delay) = state.delay.as_mut() else { return Poll::Ready(()); };
            if delay.remaining == 0 { return Poll::Ready(()); }
            if !delay.waker.as_ref().is_some_and(|waker| waker.will_wake(cx.waker())) { delay.waker = Some(cx.waker().clone()); }
            Poll::Pending
        }).await;
    }
    pub(crate) fn fail(&self, error: AdmissionError) {
        let waker = {
            let mut state = self.state.borrow_mut();
            state.failure = Some(error);
            state.waker.take()
        };
        if let Some(waker) = waker { waker.wake(); }
    }
    /// Unmatched publications remain with the caller.
    pub fn publication<M: Mmio, P: Platform, I: InterruptController>(&self, publication: Publication, lens: &LensQueueRuntime<M, P, I>) -> bool {
        let (callback, operation, reply) = match publication {
            Publication::Command { callback, operation, status, data } => (callback, Some(operation.request_id()), Reply::Command { status, data }),
            Publication::Read { callback, status, page } => (callback, None, Reply::Read { status, page: PageNumber(page.get()) }),
            Publication::Write { callback, status, next } => (callback, None, Reply::Write { status, page: PageNumber(next.get()) }),
            _ => return false,
        };
        let waker = {
            let mut state = self.state.borrow_mut();
            let Some(pending) = state.pending.as_ref() else { return false; };
            if pending.callback != callback || pending.reply.is_some() || pending.operation != operation { return false; }
            let expected = match reply {
                Reply::Read { .. } => LensCompletion::Read,
                Reply::Write { .. } => LensCompletion::Write,
                _ => pending.completion,
            };
            if expected != pending.completion { return false; }
            if matches!(reply, Reply::Read { status: Status::Complete, .. }) {
                let data = lens.read_data();
                if data.len() > state.received.len() { state.failure = Some(AdmissionError::Transport); }
                else {
                    state.received[..data.len()].copy_from_slice(&data);
                    state.received_bytes = data.len();
                }
            }
            state.pending.as_mut().unwrap().reply = Some(reply);
            state.waker.take()
        };
        if let Some(waker) = waker { waker.wake(); }
        true
    }
    pub fn signal(&self, callback: CallbackId, completion: LensCompletion, result: MediumStatus) -> bool {
        let waker = {
            let mut state = self.state.borrow_mut();
            let slot = if completion == LensCompletion::Dma { &mut state.dma } else { &mut state.pending };
            let Some(pending) = slot.as_mut() else { return false; };
            if pending.callback != callback || pending.completion != completion || pending.reply.is_some() { return false; }
            pending.reply = Some(Reply::Signal(result));
            state.waker.take()
        };
        if let Some(waker) = waker { waker.wake(); }
        true
    }
    pub fn active_callback(&self) -> Option<CallbackId> {
        let state = self.state.borrow();
        state.pending.as_ref().or(state.dma.as_ref()).map(|pending| pending.callback)
    }
}
struct CancelDelay<'a>(&'a EepromEvents);
impl Drop for CancelDelay<'_> {
    fn drop(&mut self) { self.0.state.borrow_mut().delay = None; }
}

pub struct EepromTask100<'a, 'configuration, M: Mmio, P: Platform, I: InterruptController, N: NandSoftware, D: DefaultStorageIo, B: DetectionIo> {
    pub(crate) lens: &'a RefCell<LensQueueRuntime<M, P, I>>,
    pub(crate) events: &'a EepromEvents,
    pub(crate) configuration: &'a mut ConfigImages<'configuration>,
    pub(crate) profile: &'a mut ProfileMedium<N, D>,
    pub(crate) task99: xm1_lens_transport::control_runtime::Task99Client<'a>,
    pub(crate) startup: StartupProjection,
    pub(crate) detection: DetectionProvider<'a, B>,
    pub(crate) last_load_diagnostic: Option<xm1_power_services::storage_shutdown_load::LoadDiagnostic>,
}
impl<'a, 'configuration, M: Mmio, P: Platform, I: InterruptController, N: NandSoftware, D: DefaultStorageIo, B: DetectionIo> EepromTask100<'a, 'configuration, M, P, I, N, D, B> {
    pub fn new(lens: &'a RefCell<LensQueueRuntime<M, P, I>>, events: &'a EepromEvents, configuration: &'a mut ConfigImages<'configuration>, profile: &'a mut ProfileMedium<N, D>, task99: xm1_lens_transport::control_runtime::Task99Client<'a>, startup: StartupProjection, detection: DetectionProvider<'a, B>) -> Result<Self, AdmissionError> {
        configuration.encoded_region_mut(ConfigSpan::new(0, crate::backend::CONFIG_BYTES).map_err(|_| AdmissionError::Configuration)?).map_err(|_| AdmissionError::Configuration)?;
        Ok(Self { lens, events, configuration, profile, task99, startup, detection, last_load_diagnostic: None })
    }
    pub fn last_load_diagnostic(&self) -> Option<xm1_power_services::storage_shutdown_load::LoadDiagnostic> { self.last_load_diagnostic }
    pub(crate) fn control(&mut self, operation: RequestId, words: &[u32], completion: LensCompletion) {
        let result = (|| {
            let callback = self.events.arm_command(operation, completion)?;
            let mut bytes = [0u8; 48];
            if words.len() > 10 { return Err(AdmissionError::Transport); }
            bytes[..4].copy_from_slice(&callback.0.to_le_bytes());
            for (index, word) in words.iter().enumerate() { bytes[4 + index * 4..8 + index * 4].copy_from_slice(&word.to_le_bytes()); }
            bytes[44] = words.len() as u8;
            let packet = QueueRequest::parse(operation, &bytes).map_err(|_| AdmissionError::Transport)?;
            self.lens.borrow_mut().submit(packet, None).map_err(|_| AdmissionError::Transport)
        })();
        if let Err(error) = result { self.events.fail(error); }
    }
    pub fn cancel(&mut self) -> Result<(), AdmissionError> {
        let detection = self.detection.cancel().map_err(|_| AdmissionError::Transport);
        let active = {
            let mut state = self.events.state.borrow_mut();
            state.waker = None;
            state.dma = None;
            state.pending.take().is_some()
        };
        let transport = if active { self.lens.borrow_mut().cancel().map_err(|_| AdmissionError::Transport) } else { Ok(()) };
        detection.and(transport)
    }
}
struct CancelWait<'wait, 'io, M: Mmio, P: Platform, I: InterruptController, B: DetectionIo> {
    lens: &'wait RefCell<LensQueueRuntime<M, P, I>>,
    events: &'wait EepromEvents,
    detection: &'wait DetectionProvider<'io, B>,
    complete: bool,
}
impl<M: Mmio, P: Platform, I: InterruptController, B: DetectionIo> Drop for CancelWait<'_, '_, M, P, I, B> {
    fn drop(&mut self) {
        if !self.complete {
            let mut state = self.events.state.borrow_mut();
            state.pending = None;
            state.waker = None;
            state.failure = None;
            drop(state);
            if self.detection.cancel().is_err() { self.events.fail_software(AdmissionError::Transport); }
            if self.lens.borrow_mut().cancel().is_err() { self.events.fail_software(AdmissionError::Transport); }
        }
    }
}
impl<M: Mmio, P: Platform, I: InterruptController, N: NandSoftware, D: DefaultStorageIo, B: DetectionIo> LensPageSoftware for EepromTask100<'_, '_, M, P, I, N, D, B> {
    fn clear_notifications(&mut self) {
        let mut state = self.events.state.borrow_mut();
        if state.pending.is_some() { state.failure = Some(AdmissionError::Busy); }
        state.received_bytes = 0;
        if let Some(dma) = state.dma.as_mut() { dma.reply = None; }
    }
    fn request_write(&mut self, page: PageNumber, address: ConfigurationAddress, operation: LensPageOperation) {
        let result = (|| {
            let callback = self.events.arm(LensCompletion::Write)?;
            let mut packet = [0u8; 20];
            packet[..2].copy_from_slice(&page.0.to_le_bytes());
            packet[4..8].copy_from_slice(&address.0.to_le_bytes());
            packet[8..12].copy_from_slice(&operation.abi().to_le_bytes());
            packet[12..16].copy_from_slice(&1u32.to_le_bytes());
            packet[16..20].copy_from_slice(&callback.0.to_le_bytes());
            let request = QueueRequest::parse(RequestId(0x7252), &packet).map_err(|_| AdmissionError::Transport)?;
            let length = usize::from(operation.bytes());
            let mut data = [0u8; 0x800];
            for (index, byte) in data[..length].iter_mut().enumerate() {
                *byte = self.configuration.byte(address.0.checked_add(index as u32).ok_or(AdmissionError::Configuration)?).ok_or(AdmissionError::Configuration)?;
            }
            self.lens.borrow_mut().submit(request, Some(&data[..length])).map_err(|_| AdmissionError::Transport)
        })();
        if let Err(error) = result { self.events.fail(error); }
    }
    fn request_read(&mut self, page: PageNumber, operation: LensPageOperation) {
        let result = (|| {
            let callback = self.events.arm(LensCompletion::Read)?;
            let mut packet = [0u8; 20];
            packet[..2].copy_from_slice(&page.0.to_le_bytes());
            packet[4..8].copy_from_slice(&operation.abi().to_le_bytes());
            packet[8..12].copy_from_slice(&1u32.to_le_bytes());
            packet[12..16].copy_from_slice(&1u32.to_le_bytes());
            packet[16..20].copy_from_slice(&callback.0.to_le_bytes());
            let request = QueueRequest::parse(RequestId(0x7254), &packet).map_err(|_| AdmissionError::Transport)?;
            self.lens.borrow_mut().submit(request, None).map_err(|_| AdmissionError::Transport)
        })();
        if let Err(error) = result { self.events.fail(error); }
    }
    async fn wait(&mut self, completion: LensCompletion, context: &mut [u8; 0xc0]) -> MediumStatus {
        let mut guard = CancelWait { lens: self.lens, events: self.events, detection: &self.detection, complete: false };
        let reply = poll_fn(|cx| {
            let mut state = self.events.state.borrow_mut();
            if state.failure.is_some() { return Poll::Ready(None); }
            let now = state.ticks;
            let slot = if completion == LensCompletion::Dma { &mut state.dma } else { &mut state.pending };
            let Some(pending) = slot.as_mut() else { return Poll::Ready(None); };
            if pending.completion != completion { return Poll::Ready(None); }
            if !pending.waiting { pending.waiting = true; pending.started = now; }
            if let Some(reply) = pending.reply.take() {
                if completion == LensCompletion::Dma { pending.waiting = false; } else { *slot = None; }
                state.waker = None;
                return Poll::Ready(Some(reply));
            }
            if !state.waker.as_ref().is_some_and(|waker| waker.will_wake(cx.waker())) { state.waker = Some(cx.waker().clone()); }
            Poll::Pending
        }).await;
        let Some(reply) = reply else { return MediumStatus(-255); };
        let (status, disposition) = match reply {
            Reply::Command { status, data } => {
                let status = MediumStatus(if status == Status::Complete { 0 } else { -255 });
                context[0x14..0x18].copy_from_slice(&status.0.to_le_bytes());
                if status.0 == 0 {
                    if let ReplyData::Channel { pages, format, present } = data {
                        context[0x30..0x32].copy_from_slice(&pages.to_le_bytes());
                        context[0x3a] = format;
                        context[0x3b] = u8::from(present);
                    }
                }
                (status, None)
            }
            Reply::Read { status, page } => {
                let status = MediumStatus(if status == Status::Complete { 0 } else { -255 });
                let disposition = match storage_shutdown_pages::read_completion(context, status, page, 0) { 0 => PageDisposition::Reject, 1 => PageDisposition::Accept, _ => PageDisposition::Stop };
                (MediumStatus(0), Some(disposition))
            }
            Reply::Write { status, page } => {
                let status = MediumStatus(if status == Status::Complete { 0 } else { -255 });
                let accepted = storage_shutdown_pages::write_completion(context, status, page);
                (MediumStatus(0), Some(if accepted { PageDisposition::Accept } else { PageDisposition::Reject }))
            }
            Reply::Signal(status) => (status, None),
        };
        if let Some(disposition) = disposition {
            if self.lens.borrow_mut().complete_page(disposition).is_err() { return MediumStatus(-255); }
        }
        guard.complete = true;
        status
    }
    async fn page_boundary(&mut self, boundary: PageBoundary) -> MediumStatus {
        let mut context = [0; 0xc0];
        match boundary {
            PageBoundary::Primary => self.wait(LensCompletion::Dma, &mut context).await,
            PageBoundary::Auxiliary => xm1_power_services::storage_shutdown_lifecycle::wait_dma(self, &mut context).await,
        }
    }
    fn copy_received(&mut self, _source: u32, destination: &mut [u8]) {
        let state = self.events.state.borrow();
        if destination.len() > state.received_bytes { drop(state); self.events.fail_software(AdmissionError::Transport); return; }
        destination.copy_from_slice(&state.received[..destination.len()]);
    }
    fn copy_to_configuration(&mut self, address: ConfigurationAddress, source: &[u8]) {
        let result = ConfigSpan::new(address.0, source.len() as u32).and_then(|span| self.configuration.write(span, source));
        if result.is_err() { self.events.fail_software(AdmissionError::Configuration); }
    }
}

#[cfg(test)]
#[path = "eeprom_task100_tests.rs"]
mod tests;
