//! Queue 0x62 consumer: 0x006b9c68, 0x006b9b8c and table 0x007cbe00.
//! Control and completion semantics are byte-derived from X-M1 1.50 image 4,
//! authenticated extent f2e2acab892dafeed85171f6870a411f0a2d5eae10921b86a24630d401dcff16,
//! with source/index pin 55436fc85c0560dd252b0410af6f2427fe198cd3.

#[path = "lens_queue_packets.rs"]
pub mod packets;
#[path = "lens_queue_replies.rs"]
pub mod replies;
#[path = "lens_queue_configuration.rs"]
pub mod configuration;

use core::cell::{Ref, RefCell};
use xm1_power_services::shutdown::RequestId;
use xm1_power_services::storage_shutdown_lens_control::{skip_command, LensCommandWord};
use xm1_power_services::storage_shutdown_lens_machine::{LensServiceEvent, LensServiceHandlers, LensServiceMachine, LensServiceState, LensServiceTrace};
use xm1_power_services::storage_shutdown_lens_ring::{LensCommandRing, LensPendingCommand};
use xm1_power_services::storage_shutdown_lens_service::LensCallback;
use xm1_lens_transport::{CallbackDelay, CompletionSink, CompletionStatus, PageAcceptance, RequestToken, TimingParameters, TransferOption, Transport};
use xm1_lens_transport::frame::{ChecksumMode, CommandWord, PageId, PageWriteCompletion, ReadMarker, MAX_PAYLOAD_LENGTH};
use xm1_lens_transport::wire::{CpuBackend, Error as WireError, InterruptController, Mmio, Platform, TimerToken, TransferToken};
use packets::{CallbackId, ControlOperation, QueueRequest, ReadRequest, WriteRequest};
use replies::ReplyData;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status { Complete, Stopped, InvalidReadChecksum, InvalidWriteMarker, Driver(u32) }
impl Status {
    pub const fn driver_word(self) -> u32 { match self { Self::Complete => 0, Self::Stopped => 1, Self::InvalidReadChecksum => 2, Self::InvalidWriteMarker => 3, Self::Driver(value) => value } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageDisposition { Reject, Accept, Stop }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Publication {
    Lifecycle { callback: CallbackId },
    Command { callback: CallbackId, operation: ControlOperation, status: Status, data: ReplyData },
    Write { callback: CallbackId, status: Status, next: PageId },
    Read { callback: CallbackId, status: Status, page: PageId },
    Observer { callback: CallbackId, status: Status, flags: [bool; 9] },
}
#[derive(Debug, Eq, PartialEq)]
pub enum RuntimeError<E> { Transport(xm1_lens_transport::Error<E>), Backpressure, PendingPage, MissingWriteData, WriteLength, QueueFull, LateCompletion, InvalidState, CallbackIdsExhausted, MissingConfiguration }

#[derive(Clone, Copy)]
enum WireEvent {
    Command(RequestToken, CommandWord),
    Write(RequestToken, PageWriteCompletion),
    Read(RequestToken, PageId),
    Terminated(RequestToken, CompletionStatus),
    Failed,
}
struct MailboxData {
    events: [Option<WireEvent>; 24],
    read: usize,
    count: usize,
    bytes: [u8; MAX_PAYLOAD_LENGTH + 1],
    length: usize,
    overflow: bool,
}
struct Mailbox(RefCell<MailboxData>);
impl Mailbox {
    fn new() -> Self { Self(RefCell::new(MailboxData { events: [None; 24], read: 0, count: 0, bytes: [0; MAX_PAYLOAD_LENGTH + 1], length: 0, overflow: false })) }
    fn push(&self, event: WireEvent) {
        let mut data = self.0.borrow_mut();
        if data.count == data.events.len() { data.overflow = true; return; }
        let index = (data.read + data.count) % data.events.len();
        data.events[index] = Some(event);
        data.count += 1;
    }
    fn pop(&self) -> Option<WireEvent> {
        let mut data = self.0.borrow_mut();
        if data.count == 0 { return None; }
        let index = data.read;
        let result = data.events[index].take();
        data.read = (index + 1) % data.events.len();
        data.count -= 1;
        result
    }
    fn clear(&self) { let mut data = self.0.borrow_mut(); data.events.fill(None); data.read = 0; data.count = 0; data.overflow = false; }
}
impl CompletionSink for Mailbox {
    fn command_completed(&mut self, token: RequestToken, response: CommandWord) -> CallbackDelay { self.push(WireEvent::Command(token, response)); CallbackDelay::Normal }
    fn page_written(&mut self, token: RequestToken, response: PageWriteCompletion) { self.push(WireEvent::Write(token, response)); }
    fn page_read(&mut self, token: RequestToken, page: PageId, bytes: &[u8]) {
        let mut data = self.0.borrow_mut();
        if bytes.len() > data.bytes.len() { data.overflow = true; return; }
        data.bytes[..bytes.len()].copy_from_slice(bytes);
        data.length = bytes.len();
        drop(data);
        self.push(WireEvent::Read(token, page));
    }
    fn terminated(&mut self, token: RequestToken, status: CompletionStatus) { self.push(WireEvent::Terminated(token, status)); }
    fn transport_failed(&mut self) { self.push(WireEvent::Failed); }
}

struct PendingRing {
    slots: [LensPendingCommand; 16], tokens: [Option<RequestToken>; 16], write: u32, read: u32, count: u32,
}
impl PendingRing {
    fn new() -> Self { Self { slots: [LensPendingCommand::new(RequestId(0x7202), false, 0, LensCallback(0), false); 16], tokens: [None; 16], write: 0, read: 0, count: 0 } }
    fn ring(&mut self) -> LensCommandRing<'_> { LensCommandRing::new(&mut self.slots, &mut self.write, &mut self.read, &mut self.count) }
    fn push(&mut self, command: LensPendingCommand, token: Option<RequestToken>) -> bool {
        let index = self.write as usize;
        if self.ring().push(command).is_err() { return false; }
        self.tokens[index] = token;
        true
    }
    fn pop(&mut self) -> Option<(LensPendingCommand, Option<RequestToken>)> { let index = self.read as usize; let command = self.ring().pop()?; Some((command, self.tokens[index].take())) }
    fn peek(&mut self) -> Option<LensPendingCommand> { self.ring().peek() }
    fn clear(&mut self) { self.tokens = [None; 16]; self.write = 0; self.read = 0; self.count = 0; }
}
#[derive(Clone, Copy)]
enum PendingPage { Read(RequestToken, ReadRequest), Write(RequestToken, WriteRequest) }
impl PendingPage { fn token(self) -> RequestToken { match self { Self::Read(token, _) | Self::Write(token, _) => token } } }

struct Consumer<M: Mmio, P: Platform, I: InterruptController> {
    transport: Transport<CpuBackend<M, P, I>, Mailbox>,
    context: [u8; 0x174],
    initial_capabilities: [u8; 64],
    source_configuration: Option<configuration::LensSourceConfiguration>,
    secondary_timing: Option<xm1_lens_transport::TimingProfile>,
    pending: PendingRing,
    page: Option<PendingPage>,
    awaiting_acceptance: bool,
    observer: CallbackId,
    next_callback: CallbackId,
    observer_flags: [bool; 9],
    outputs: [Option<Publication>; 64],
    output_read: usize,
    output_count: usize,
    error: Option<RuntimeError<WireError<P::Error, I::Error>>>,
}
impl<M: Mmio, P: Platform, I: InterruptController> Consumer<M, P, I> {
    fn publish(&mut self, output: Publication) { let index = (self.output_read + self.output_count) % self.outputs.len(); self.outputs[index] = Some(output); self.output_count += 1; }
    fn lifecycle(&mut self, callback: CallbackId) { if callback.0 != 0 { self.publish(Publication::Lifecycle { callback }); } }
    fn observer(&mut self, status: Status) { if self.observer.0 != 0 { self.publish(Publication::Observer { callback: self.observer, status, flags: self.observer_flags }); } }
    fn reply(&mut self, command: LensPendingCommand, status: Status, response: u32) {
        if !command.is_last() || command.callback.0 == 0 { return; }
        let Some(operation) = ControlOperation::parse(command.operation()) else { return; };
        let Some(data) = replies::decode(operation, response) else { return; };
        if let ReplyData::ObserverFlags(flags) = data { self.observer_flags = flags; }
        self.publish(Publication::Command { callback: CallbackId(command.callback.0), operation, status, data });
    }
    fn reset(&mut self) {
        if let Err(error) = self.transport.stop() { self.error = Some(RuntimeError::Transport(error)); }
        self.transport.completions().clear();
        while let Some((command, _)) = self.pending.pop() { self.reply(command, Status::Stopped, 0); }
        if let Some(page) = self.page.take() {
            match page {
                PendingPage::Read(_, request) => if request.callback.0 != 0 { self.publish(Publication::Read { callback: request.callback, status: Status::InvalidReadChecksum, page: PageId::new(0) }); },
                PendingPage::Write(_, request) => if request.callback.0 != 0 { self.publish(Publication::Write { callback: request.callback, status: Status::InvalidWriteMarker, next: PageId::new(0) }); },
            }
        }
        self.context[0xb4..0xdc].fill(0);
        self.context[0xdc] = 0;
        self.awaiting_acceptance = false;
    }
    fn control(&mut self, packet: packets::ControlPacket) {
        let operation = packet.operation.request_id();
        let words = packet.words();
        let required = words.len() + usize::from(words.last().is_some_and(|word| matches!(word.command_id(), 0x62 | 0x56 | 0x58 | 0x40 | 0x80 | 0xa8 | 0xa9)));
        if self.pending.count as usize + required > 16 { self.error = Some(RuntimeError::QueueFull); return; }
        for (index, word) in words.iter().enumerate() {
            let last = index + 1 == words.len();
            let skipped = skip_command(&self.context, operation, LensCommandWord(word.raw()));
            let command = LensPendingCommand::new(operation, last, word.raw(), LensCallback(packet.callback.0), skipped);
            if skipped {
                if self.pending.count == 0 { self.reply(command, Status::Complete, 0); } else { self.pending.push(command, None); }
            } else {
                match self.transport.enqueue_command(*word) {
                    Ok(token) => { self.pending.push(command, Some(token)); }
                    Err(error) => { self.error = Some(RuntimeError::Transport(error)); return; }
                }
                if last && matches!(word.command_id(), 0x62 | 0x56 | 0x58 | 0x40 | 0x80 | 0xa8 | 0xa9) {
                    match self.transport.enqueue_command(CommandWord::new(0x3fc0)) {
                        Ok(token) => { self.pending.push(LensPendingCommand::new(RequestId(0x7202), true, 0x3fc0, LensCallback(0), false), Some(token)); }
                        Err(error) => { self.error = Some(RuntimeError::Transport(error)); return; }
                    }
                }
            }
        }
    }
    fn command_response(&mut self, token: RequestToken, response: CommandWord, status: Status) -> Option<LensServiceEvent> {
        let Some(command) = self.pending.peek() else { self.error = Some(RuntimeError::LateCompletion); return None; };
        if self.pending.tokens[self.pending.read as usize] != Some(token) { self.error = Some(RuntimeError::LateCompletion); return None; }
        self.pending.pop();
        let count = usize::from(self.context[0xdc]);
        if count < 10 { self.context[0xb4 + count * 4..0xb8 + count * 4].copy_from_slice(&response.raw().to_le_bytes()); self.context[0xdc] += 1; }
        let first = u32::from_le_bytes([self.context[0xb4], self.context[0xb5], self.context[0xb6], self.context[0xb7]]);
        self.reply(command, status, first);
        if command.is_last() { self.context[0xdc] = 0; }
        while self.pending.peek().is_some_and(LensPendingCommand::is_skipped) {
            if let Some((skipped, _)) = self.pending.pop() { self.reply(skipped, status, 0); if skipped.is_last() { self.context[0xdc] = 0; } }
        }
        if command.operation().0 == 0x722b { self.context[0x12a..0x12c].copy_from_slice(&((command.command >> 16) as u16).to_le_bytes()); }
        let observer_status = if command.operation().0 == 0x7217 && status == Status::Complete && first >> 16 != 0 { Status::Driver(4) } else { status };
        if observer_status != Status::Complete { self.observer(observer_status); Some(LensServiceEvent::Stop) } else { None }
    }
}

#[derive(Clone, Copy)]
enum Input { Request(QueueRequest), Wire(WireEvent) }
struct Dispatch<'a, 'b, M: Mmio, P: Platform, I: InterruptController> { consumer: &'a mut Consumer<M, P, I>, input: Input, write: Option<&'b [u8]> }
impl<M: Mmio, P: Platform, I: InterruptController> LensServiceHandlers for Dispatch<'_, '_, M, P, I> {
    fn operation(&self) -> RequestId { match self.input { Input::Request(request) => request.operation(), Input::Wire(WireEvent::Write(..)) => RequestId(0x7253), Input::Wire(WireEvent::Read(..)) => RequestId(0x7255), _ => RequestId(0x7251) } }
    fn reset(&mut self) {
        self.consumer.reset();
        if self.consumer.error.is_none() { if let Input::Request(QueueRequest::Stop { callback }) = self.input { self.consumer.lifecycle(callback); } }
    }
    fn initialize(&mut self) {
        if let Input::Request(QueueRequest::Initialize { callback, observer }) = self.input {
            self.consumer.observer = observer;
            self.consumer.context[0x12a..0x12c].fill(0);
            self.consumer.context[0x12c..0x16c].copy_from_slice(&self.consumer.initial_capabilities);
            self.consumer.context[0x16c..0x170].copy_from_slice(&1u32.to_le_bytes());
            self.consumer.context[0x170..0x174].fill(0);
            self.consumer.observer_flags.fill(false);
            if let Some(configuration) = self.consumer.source_configuration { self.consumer.transport.set_timing(configuration.initial_timing().profile()); }
            self.consumer.secondary_timing = None;
            self.consumer.pending.clear();
            self.consumer.transport.backend_mut().initialize_detection_output();
            match self.consumer.transport.initialize() { Ok(()) => self.consumer.lifecycle(callback), Err(error) => self.consumer.error = Some(RuntimeError::Transport(error)) }
        }
    }
    fn stopped_command(&mut self) {
        if let Input::Request(QueueRequest::Control(packet)) = self.input {
            if !packet.words().is_empty() { self.consumer.reply(LensPendingCommand::new(packet.operation.request_id(), true, 0, LensCallback(packet.callback.0), false), Status::Stopped, 0); }
        }
    }
    fn stopped_write(&mut self) { if let Input::Request(QueueRequest::Write(request)) = self.input { if request.callback.0 != 0 { self.consumer.publish(Publication::Write { callback: request.callback, status: Status::InvalidWriteMarker, next: PageId::new(0) }); } } }
    fn stopped_read(&mut self) { if let Input::Request(QueueRequest::Read(request)) = self.input { if request.callback.0 != 0 { self.consumer.publish(Publication::Read { callback: request.callback, status: Status::InvalidReadChecksum, page: PageId::new(0) }); } } }
    fn queue_control(&mut self) { if let Input::Request(QueueRequest::Control(packet)) = self.input { self.consumer.control(packet); } }
    fn command_response(&mut self) -> Option<LensServiceEvent> {
        match self.input {
            Input::Wire(WireEvent::Command(token, response)) => self.consumer.command_response(token, response, Status::Complete),
            Input::Wire(WireEvent::Terminated(token, status)) => {
                if self.consumer.page.is_some_and(|page| page.token() == token) { self.consumer.observer(Status::Stopped); return Some(LensServiceEvent::Stop); }
                self.consumer.command_response(token, CommandWord::new(0), if status == CompletionStatus::Failed { Status::Driver(1) } else { Status::Stopped })
            }
            Input::Wire(WireEvent::Failed) => { self.consumer.observer(Status::Driver(1)); Some(LensServiceEvent::Stop) }
            _ => None,
        }
    }
    fn write_page(&mut self) {
        if let Input::Request(QueueRequest::Write(request)) = self.input {
            if self.consumer.page.is_some() || self.consumer.awaiting_acceptance { self.consumer.error = Some(RuntimeError::PendingPage); return; }
            let Some(bytes) = self.write else { self.consumer.error = Some(RuntimeError::MissingWriteData); return; };
            if bytes.len() != request.length.get() { self.consumer.error = Some(RuntimeError::WriteLength); return; }
            match self.consumer.transport.write_page(request.page, bytes, if request.checksum { ChecksumMode::Enabled } else { ChecksumMode::Disabled }) {
                Ok(token) => self.consumer.page = Some(PendingPage::Write(token, request)), Err(error) => self.consumer.error = Some(RuntimeError::Transport(error)),
            }
        }
    }
    fn write_response(&mut self) -> Option<LensServiceEvent> {
        if let Input::Wire(WireEvent::Write(token, response)) = self.input {
            let Some(PendingPage::Write(expected, request)) = self.consumer.page else { self.consumer.error = Some(RuntimeError::LateCompletion); return None; };
            if token != expected { self.consumer.error = Some(RuntimeError::LateCompletion); return None; }
            self.consumer.page = None;
            let status = if response.marker_valid { Status::Complete } else { Status::InvalidWriteMarker };
            if request.callback.0 != 0 { self.consumer.awaiting_acceptance = true; self.consumer.publish(Publication::Write { callback: request.callback, status, next: response.page }); }
            else if let Err(error) = self.consumer.transport.accept_page(PageAcceptance::Accepted) { self.consumer.error = Some(RuntimeError::Transport(error)); }
        }
        None
    }
    fn read_page(&mut self) {
        if let Input::Request(QueueRequest::Read(request)) = self.input {
            if self.consumer.page.is_some() || self.consumer.awaiting_acceptance { self.consumer.error = Some(RuntimeError::PendingPage); return; }
            match self.consumer.transport.read_page(request.page, request.length, if request.checksum { TransferOption::ExtraByte } else { TransferOption::Plain }, if request.marker { ReadMarker::Enabled } else { ReadMarker::Disabled }) {
                Ok(token) => self.consumer.page = Some(PendingPage::Read(token, request)), Err(error) => self.consumer.error = Some(RuntimeError::Transport(error)),
            }
        }
    }
    fn read_response(&mut self) -> Option<LensServiceEvent> {
        if let Input::Wire(WireEvent::Read(token, page)) = self.input {
            let Some(PendingPage::Read(expected, request)) = self.consumer.page else { self.consumer.error = Some(RuntimeError::LateCompletion); return None; };
            if token != expected { self.consumer.error = Some(RuntimeError::LateCompletion); return None; }
            self.consumer.page = None;
            let status = {
                let data = self.consumer.transport.completions().0.borrow();
                if request.checksum {
                    let length = request.length.get();
                    let sum = (page.get() as u8).wrapping_add((page.get() >> 8) as u8).wrapping_add(data.bytes[..length].iter().fold(0u8, |sum, byte| sum.wrapping_add(*byte)));
                    if data.length <= length || data.bytes[length] != sum { Status::InvalidReadChecksum } else { Status::Complete }
                } else { Status::Complete }
            };
            if request.callback.0 != 0 { self.consumer.awaiting_acceptance = true; self.consumer.publish(Publication::Read { callback: request.callback, status, page }); }
            else if let Err(error) = self.consumer.transport.accept_page(PageAcceptance::Accepted) { self.consumer.error = Some(RuntimeError::Transport(error)); }
        }
        None
    }
    fn invalid_state(&mut self, _: u32, _: LensServiceEvent) { self.consumer.error = Some(RuntimeError::InvalidState); }
    fn unexpected_initialize(&mut self, _: LensServiceState) { self.consumer.error = Some(RuntimeError::InvalidState); }
}

pub struct LensQueueRuntime<M: Mmio, P: Platform, I: InterruptController> {
    history: [u32; 4], trace: [LensServiceTrace; 16], cursor: u32, consumer: Consumer<M, P, I>,
}
impl<M: Mmio, P: Platform, I: InterruptController> LensQueueRuntime<M, P, I> {
    pub fn new(backend: CpuBackend<M, P, I>, timing: TimingParameters, configuration_capabilities: [u8; 64]) -> Self {
        Self { history: [1, 0, 0, 0], trace: [LensServiceTrace { state: 0, event: 0 }; 16], cursor: 0,
            consumer: Consumer { transport: Transport::new(backend, Mailbox::new(), timing), context: [0; 0x174], initial_capabilities: configuration_capabilities, source_configuration: None, secondary_timing: None, pending: PendingRing::new(), page: None, awaiting_acceptance: false, observer: CallbackId(0), next_callback: CallbackId(0x1000_0000), observer_flags: [false; 9], outputs: [None; 64], output_read: 0, output_count: 0, error: None } }
    }
    pub fn from_configuration(backend: CpuBackend<M, P, I>, configuration: configuration::LensSourceConfiguration) -> Self {
        let mut runtime = Self::new(backend, configuration.initial_timing(), configuration.supported.0);
        runtime.consumer.source_configuration = Some(configuration);
        runtime
    }
    pub fn finish_configuration(&mut self, model: configuration::LensModelIdentifier) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> {
        let configuration = self.consumer.source_configuration.ok_or(RuntimeError::MissingConfiguration)?;
        let finished = configuration.finish(model);
        self.consumer.context[0x12c..0x16c].copy_from_slice(&finished.supported.0);
        self.consumer.context[0x16c..0x170].copy_from_slice(&u32::from(!finished.restrict_extended_mode).to_le_bytes());
        self.consumer.context[0x170..0x174].copy_from_slice(&1u32.to_le_bytes());
        self.consumer.transport.set_timing(finished.primary);
        self.consumer.secondary_timing = Some(finished.secondary);
        Ok(())
    }
    pub fn secondary_timing(&self) -> Option<xm1_lens_transport::TimingProfile> { self.consumer.secondary_timing }
    pub fn prepare_service(&mut self) {
        self.consumer.transport.backend_mut().prepare_detection_output();
        self.consumer.context[0xa8..0xac].fill(0);
    }
    pub fn state(&self) -> LensServiceState { if self.history[0] == 2 { LensServiceState::Running } else { LensServiceState::Stopped } }
    pub fn driver(&self) -> &CpuBackend<M, P, I> { self.consumer.transport.backend() }
    pub fn allocate_callback(&mut self) -> Result<CallbackId, RuntimeError<WireError<P::Error, I::Error>>> {
        let callback = self.consumer.next_callback;
        self.consumer.next_callback = CallbackId(callback.0.checked_add(1).ok_or(RuntimeError::CallbackIdsExhausted)?);
        Ok(callback)
    }
    pub fn submit(&mut self, request: QueueRequest, write: Option<&[u8]>) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> {
        self.admit()?;
        self.dispatch(Input::Request(request), write);
        if let Some(error) = self.consumer.error.take() {
            if self.state() == LensServiceState::Running && matches!(&error, RuntimeError::Transport(_)) { self.dispatch(Input::Request(QueueRequest::Stop { callback: CallbackId(0) }), None); self.consumer.error = None; }
            return Err(error);
        }
        Ok(())
    }
    fn admit(&self) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> { if self.consumer.output_count > 32 { Err(RuntimeError::Backpressure) } else { Ok(()) } }
    fn dispatch(&mut self, input: Input, write: Option<&[u8]>) {
        LensServiceMachine::new(&mut self.history, &mut self.trace, &mut self.cursor).dispatch(&mut Dispatch { consumer: &mut self.consumer, input, write }, LensServiceEvent::Received);
    }
    fn drain(&mut self) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> {
        while let Some(event) = self.consumer.transport.completions().pop() { self.dispatch(Input::Wire(event), None); if let Some(error) = self.consumer.error.take() { return Err(error); } }
        if self.consumer.transport.completions().0.borrow().overflow { return Err(RuntimeError::Backpressure); }
        Ok(())
    }
    pub fn wire_completed(&mut self, token: TransferToken) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> {
        self.admit()?;
        let mut bytes = [0u8; xm1_lens_transport::wire::MAX_TRANSFER_BYTES];
        let length = self.consumer.transport.backend_mut().complete_wire(token, &mut bytes).map_err(|error| RuntimeError::Transport(xm1_lens_transport::Error::Backend(error)))?;
        let result = self.consumer.transport.receive(token, &bytes[..length]).map_err(RuntimeError::Transport);
        self.drain()?;
        result
    }
    pub fn timer_expired(&mut self, token: TimerToken) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> {
        self.admit()?;
        let result = self.consumer.transport.timer_expired(token).map_err(RuntimeError::Transport);
        self.drain()?;
        result
    }
    pub fn complete_page(&mut self, disposition: PageDisposition) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> {
        self.admit()?;
        if !self.consumer.awaiting_acceptance { return Err(RuntimeError::LateCompletion); }
        self.consumer.awaiting_acceptance = false;
        match disposition {
            PageDisposition::Accept | PageDisposition::Reject => self.consumer.transport.accept_page(if disposition == PageDisposition::Accept { PageAcceptance::Accepted } else { PageAcceptance::Rejected }).map_err(RuntimeError::Transport),
            PageDisposition::Stop => { self.consumer.observer(Status::Stopped); self.dispatch(Input::Request(QueueRequest::Stop { callback: CallbackId(0) }), None); self.consumer.error.take().map_or(Ok(()), Err) }
        }
    }
    pub fn cancel(&mut self) -> Result<(), RuntimeError<WireError<P::Error, I::Error>>> { self.submit(QueueRequest::Stop { callback: CallbackId(0) }, None) }
    pub fn pop_publication(&mut self) -> Option<Publication> {
        if self.consumer.output_count == 0 { return None; }
        let index = self.consumer.output_read;
        self.consumer.output_read = (index + 1) % self.consumer.outputs.len();
        self.consumer.output_count -= 1;
        self.consumer.outputs[index].take()
    }
    pub fn read_data(&self) -> Ref<'_, [u8]> { Ref::map(self.consumer.transport.completions().0.borrow(), |data| &data.bytes[..data.length]) }
    pub fn awaiting_page_acceptance(&self)->bool { self.consumer.awaiting_acceptance }
}

#[cfg(test)]
#[path = "lens_queue_runtime_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "lens_queue_physical_fixture.rs"]
pub(crate) mod physical_fixture;
