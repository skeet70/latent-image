#![no_std]

pub mod frame;
mod queue;
pub mod state;
pub mod wire;
pub mod control_actor;
pub mod control_device;
pub mod control_types;
pub mod control_runtime;
pub mod control_callbacks;
pub mod control_publication;
pub mod key_ui;
#[cfg(all(feature = "firmware-abi", target_arch = "arm"))]
pub mod control_native;
#[cfg(all(feature = "firmware-abi", target_arch = "arm"))]
pub mod control_root;
#[cfg(all(feature = "firmware-abi", target_arch = "arm"))]
pub mod key_ui_native;
#[cfg(feature = "native-codec")]
pub mod codec_abi;
#[cfg(all(feature = "firmware-abi", target_arch = "arm"))]
pub mod wire_native;
#[cfg(all(feature = "firmware-abi", target_arch = "arm"))]
pub mod abi;

use frame::*;
use queue::CommandQueue;
pub use queue::QueueError;
use state::{Event, Handler, State};
use wire::{Backend, TimerTicks, TimerToken, TransferToken};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RequestToken(u32);
impl RequestToken { pub const fn get(self) -> u32 { self.0 } }

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GenerationCheckpoint(u32);
impl GenerationCheckpoint {
    pub const fn get(self) -> u32 { self.0 }
    pub(crate) const fn from_abi(value: u32) -> Self { Self(value) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompletionStatus { Complete, Failed, Cancelled, Reset, Teardown }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallbackDelay { Normal, Deferred }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PageAcceptance { Accepted, Rejected }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferOption { Plain, ExtraByte }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMode { Notify, DiagnosticBlink }

pub trait CompletionSink {
    fn command_completed(&mut self, token: RequestToken, response: CommandWord) -> CallbackDelay;
    fn page_written(&mut self, token: RequestToken, response: PageWriteCompletion);
    fn page_read(&mut self, token: RequestToken, page: PageId, bytes: &[u8]);
    fn terminated(&mut self, token: RequestToken, status: CompletionStatus);
    fn transport_failed(&mut self);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingProfile {
    pub command: TimerTicks,
    pub normal: TimerTicks,
    pub deferred: TimerTicks,
    pub accepted_page: TimerTicks,
    pub rejected_page: TimerTicks,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TimingParameters {
    pub command: TimerTicks,
    pub normal: TimerTicks,
    pub deferred: TimerTicks,
    pub accepted_page: TimerTicks,
    pub rejected_page: TimerTicks,
    pub recovery: TimerTicks,
    pub recovery_limit: RetryLimit,
    pub busy_limit: RetryLimit,
    pub failure_mode: FailureMode,
}
impl TimingParameters {
    pub const fn profile(self) -> TimingProfile {
        TimingProfile { command: self.command, normal: self.normal, deferred: self.deferred, accepted_page: self.accepted_page, rejected_page: self.rejected_page }
    }
    fn apply_profile(&mut self, profile: TimingProfile) {
        self.command = profile.command;
        self.normal = profile.normal;
        self.deferred = profile.deferred;
        self.accepted_page = profile.accepted_page;
        self.rejected_page = profile.rejected_page;
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum Error<E> {
    Backend(E),
    Frame(FrameError),
    Queue(QueueError),
    Unexpected { state: State, event: Event },
    Busy,
    Stopped,
    LateCompletion,
    TokenExhausted,
}

#[derive(Clone, Copy)]
enum DelaySelection { Normal, Deferred, AcceptedPage, RejectedPage }
#[derive(Clone, Copy)]
enum PageRequest {
    Write { token: RequestToken, length: PayloadLength, marker: ReadMarker },
    Read { token: RequestToken, length: PayloadLength, option: TransferOption },
}
impl PageRequest {
    fn token(self) -> RequestToken { match self { Self::Write { token, .. } | Self::Read { token, .. } => token } }
}

#[derive(Clone, Copy)]
struct PendingCommand { token: RequestToken, sequence: Sequence }

pub struct Transport<B: Backend, C: CompletionSink> {
    backend: B,
    completions: C,
    state: State,
    previous_states: [State; 3],
    registered: bool,
    timing: TimingParameters,
    replacement_timing: Option<TimingProfile>,
    queue: CommandQueue,
    queued_sequence: Sequence,
    expected_sequence: Sequence,
    outstanding: [Option<PendingCommand>; 20],
    command: CommandWord,
    current_command: CommandWord,
    previous_command: CommandWord,
    acknowledgement: CommandWord,
    last_response: Option<CommandWord>,
    recovery: RecoveryMode,
    retry_count: RetryCount,
    busy_count: RetryCount,
    delay: DelaySelection,
    page: Option<PageRequest>,
    page_delivered: bool,
    frame: OwnedFrame<MAX_FRAME_LENGTH>,
    transfer: Option<TransferToken>,
    timer: Option<TimerToken>,
    generation: GenerationCheckpoint,
}

impl<B: Backend, C: CompletionSink> Transport<B, C> {
    pub fn new(backend: B, completions: C, timing: TimingParameters) -> Self {
        Self::resume(backend, completions, timing, GenerationCheckpoint::default())
    }

    pub fn resume(backend: B, completions: C, timing: TimingParameters, generation: GenerationCheckpoint) -> Self {
        Self {
            backend, completions, timing,
            state: State::Stopped, previous_states: [State::Stopped; 3], registered: false,
            replacement_timing: None, queue: CommandQueue::new(),
            queued_sequence: Sequence::new(0).unwrap(), expected_sequence: Sequence::new(0).unwrap(),
            outstanding: [None; 20], command: CommandWord::new(0), current_command: CommandWord::new(0), previous_command: CommandWord::new(0),
            acknowledgement: CommandWord::new(0), last_response: None, recovery: RecoveryMode::Normal,
            retry_count: RetryCount::new(0), busy_count: RetryCount::new(0), delay: DelaySelection::Normal,
            page: None, page_delivered: false, frame: OwnedFrame::new(), transfer: None, timer: None, generation,
        }
    }

    pub const fn state(&self) -> State { self.state }
    pub fn backend(&self) -> &B { &self.backend }
    pub fn backend_mut(&mut self) -> &mut B { &mut self.backend }
    pub fn completions(&self) -> &C { &self.completions }
    pub const fn generation_checkpoint(&self) -> GenerationCheckpoint { self.generation }

    fn generation(&mut self) -> Result<u32, Error<B::Error>> {
        self.generation.0 = self.generation.0.checked_add(1).ok_or(Error::TokenExhausted)?;
        Ok(self.generation.0)
    }

    pub fn initialize(&mut self) -> Result<(), Error<B::Error>> {
        if self.registered { return Err(Error::Busy); }
        self.queue = CommandQueue::new();
        self.queued_sequence = Sequence::new(0).unwrap();
        self.expected_sequence = Sequence::new(0).unwrap();
        self.command = CommandWord::new(0);
        self.current_command = CommandWord::new(0);
        self.previous_command = CommandWord::new(0);
        self.acknowledgement = CommandWord::new(0);
        self.last_response = None;
        self.recovery = RecoveryMode::Normal;
        self.retry_count = RetryCount::new(0);
        self.busy_count = RetryCount::new(0);
        self.delay = DelaySelection::Normal;
        self.previous_states = [State::Stopped; 3];
        self.replacement_timing = None;
        self.dispatch(Event::Initialize, &[])
    }

    pub fn set_timing(&mut self, timing: TimingProfile) { self.timing.apply_profile(timing); }

    pub fn replace_timing(&mut self, timing: TimingProfile) {
        self.replacement_timing = Some(timing);
    }

    pub fn enqueue_command(&mut self, command: CommandWord) -> Result<RequestToken, Error<B::Error>> {
        if !self.registered || self.state == State::Stopped { return Err(Error::Stopped); }
        if matches!(self.state, State::PageWriteTransmit | State::PageReadTransmit | State::PageAcceptance) { return Err(Error::Busy); }
        let slot = self.outstanding.iter().position(Option::is_none).ok_or(Error::Queue(QueueError::Full))?;
        let token = RequestToken(self.generation()?);
        self.queue.push(QueuedCommand::new(command, self.queued_sequence)).map_err(Error::Queue)?;
        self.outstanding[slot] = Some(PendingCommand { token, sequence: self.queued_sequence });
        self.queued_sequence = self.queued_sequence.next();
        self.publish_request()?;
        Ok(token)
    }

    pub fn write_page(&mut self, page: PageId, bytes: &[u8], checksum: ChecksumMode) -> Result<RequestToken, Error<B::Error>> {
        self.admit_page()?;
        let length = self.frame.build_write(page, bytes, checksum).map_err(Error::Frame)?;
        let token = RequestToken(self.generation()?);
        self.page = Some(PageRequest::Write { token, length, marker: match checksum { ChecksumMode::Disabled => ReadMarker::Disabled, ChecksumMode::Enabled => ReadMarker::Enabled } });
        self.page_delivered = false;
        self.publish_request()?;
        Ok(token)
    }

    pub fn read_page(&mut self, page: PageId, length: PayloadLength, option: TransferOption, marker: ReadMarker) -> Result<RequestToken, Error<B::Error>> {
        self.admit_page()?;
        self.frame.build_read(page, length, marker).map_err(Error::Frame)?;
        let token = RequestToken(self.generation()?);
        self.page = Some(PageRequest::Read { token, length, option });
        self.page_delivered = false;
        self.publish_request()?;
        Ok(token)
    }

    fn publish_request(&mut self) -> Result<(), Error<B::Error>> {
        let result = self.backend.set_requested_output(true).map_err(Error::Backend).and_then(|()| self.dispatch(Event::Request, &[]));
        if result.is_err() { let _ = self.fail(); }
        result
    }

    fn admit_page(&self) -> Result<(), Error<B::Error>> {
        if !self.registered { return Err(Error::Stopped); }
        if self.page.is_some() || !matches!(self.state, State::Idle | State::CompletionDelay) { return Err(Error::Busy); }
        Ok(())
    }

    pub fn receive(&mut self, token: TransferToken, bytes: &[u8]) -> Result<(), Error<B::Error>> {
        if self.transfer != Some(token) { return Err(Error::LateCompletion); }
        self.transfer = None;
        self.dispatch(Event::WireCompleted, bytes)
    }

    pub fn timer_expired(&mut self, token: TimerToken) -> Result<(), Error<B::Error>> {
        if self.timer != Some(token) { return Err(Error::LateCompletion); }
        self.backend.cancel_timer().map_err(Error::Backend)?;
        self.timer = None;
        if let Some(timing) = self.replacement_timing.take() { self.timing.apply_profile(timing); }
        self.dispatch(Event::TimerExpired, &[])
    }

    pub fn accept_page(&mut self, acceptance: PageAcceptance) -> Result<(), Error<B::Error>> {
        if self.state != State::PageAcceptance { return Err(Error::Unexpected { state: self.state, event: Event::Accepted }); }
        self.delay = match acceptance { PageAcceptance::Accepted => DelaySelection::AcceptedPage, PageAcceptance::Rejected => DelaySelection::RejectedPage };
        self.page = None;
        self.dispatch(Event::Accepted, &[])
    }

    pub fn cancel(&mut self) -> Result<(), Error<B::Error>> { self.shutdown(CompletionStatus::Cancelled) }
    pub fn stop(&mut self) -> Result<(), Error<B::Error>> { self.shutdown(CompletionStatus::Teardown) }
    pub fn fail(&mut self) -> Result<(), Error<B::Error>> {
        self.completions.transport_failed();
        self.shutdown(CompletionStatus::Failed)
    }
    pub fn reset(&mut self) -> Result<(), Error<B::Error>> {
        self.shutdown(CompletionStatus::Reset)?;
        self.initialize()
    }

    fn shutdown(&mut self, status: CompletionStatus) -> Result<(), Error<B::Error>> {
        let mut first_error = None;
        self.transfer = None;
        self.timer = None;
        if self.registered {
            if let Err(error) = self.backend.cancel_timer() { first_error = Some(error); }
            if let Err(error) = self.backend.abort_wire() { if first_error.is_none() { first_error = Some(error); } }
            match self.backend.unregister() {
                Ok(()) => self.registered = false,
                Err(error) => if first_error.is_none() { first_error = Some(error); },
            }
        }
        self.state = State::Stopped;
        self.retire(status);
        if let Err(error) = self.backend.set_requested_output(false) { if first_error.is_none() { first_error = Some(error); } }
        match first_error { Some(error) => Err(Error::Backend(error)), None => Ok(()) }
    }

    fn retire(&mut self, status: CompletionStatus) {
        if let Some(page) = self.page.take() {
            if !self.page_delivered { self.completions.terminated(page.token(), status); }
        }
        self.page_delivered = false;
        for pending in &mut self.outstanding {
            if let Some(pending) = pending.take() { self.completions.terminated(pending.token, status); }
        }
        self.queue = CommandQueue::new();
    }

    fn dispatch(&mut self, mut event: Event, bytes: &[u8]) -> Result<(), Error<B::Error>> {
        loop {
            let transition = self.state.transition(event);
            if let Some(next_event) = self.handle(transition.handler, event, bytes)? {
                event = next_event;
                continue;
            }
            match transition.next {
                None => return Ok(()),
                Some(next) => {
                    self.previous_states.rotate_right(1);
                    self.previous_states[0] = self.state;
                    self.state = next;
                    event = Event::Enter;
                }
            }
        }
    }

    fn start_word(&mut self, word: CommandWord) -> Result<(), Error<B::Error>> {
        let token = TransferToken::new(self.generation()?).ok_or(Error::TokenExhausted)?;
        self.backend.start(token, &word.wire_bytes()).map_err(Error::Backend)?;
        self.transfer = Some(token);
        Ok(())
    }

    fn arm(&mut self, delay: TimerTicks) -> Result<(), Error<B::Error>> {
        let token = TimerToken::new(self.generation()?).ok_or(Error::TokenExhausted)?;
        self.backend.arm_timer(token, delay).map_err(Error::Backend)?;
        self.timer = Some(token);
        Ok(())
    }

    fn outcome(outcome: ResponseOutcome, recovery: RecoveryMode) -> Option<Event> {
        match outcome.stock_code(recovery) {
            0 => None,
            6 => Some(Event::Busy),
            8 => Some(Event::Invalid),
            9 => Some(Event::Recover),
            _ => Some(Event::Exhausted),
        }
    }

    fn handle(&mut self, handler: Handler, event: Event, bytes: &[u8]) -> Result<Option<Event>, Error<B::Error>> {
        use Handler::*;
        match handler {
            Noop => {},
            Unexpected => return Err(Error::Unexpected { state: self.state, event }),
            EnterStopped => {
                if self.previous_states[0] != State::Idle { self.backend.set_requested_output(false).map_err(Error::Backend)?; }
            },
            EnterIdle => self.backend.set_requested_output(false).map_err(Error::Backend)?,
            Register => {
                self.backend.register().map_err(Error::Backend)?;
                self.registered = true;
            },
            SelectRequest => {
                if let Some(page) = self.page {
                    return Ok(Some(match page { PageRequest::Write { .. } => Event::WritePage, PageRequest::Read { .. } => Event::ReadPage }));
                }
                let selected = if self.state == State::CompletionDelay && self.previous_states[0] == State::RecoveryTransmit {
                    let sequence = if self.recovery == RecoveryMode::Restart { Sequence::new((self.expected_sequence.get() + 7) & 7).unwrap() } else { self.expected_sequence };
                    Some(self.queue.rewind(sequence).map_err(Error::Queue)?)
                } else { self.queue.pop() };
                if let Some(record) = selected {
                    self.command = record.command;
                    self.acknowledgement = record.acknowledgement;
                } else if self.current_command.raw() == 0 {
                    return Ok(Some(Event::Accepted));
                } else {
                    self.command = CommandWord::new(0);
                    self.acknowledgement = CommandWord::new(self.acknowledgement.raw() & !0x0700_0000);
                }
            },
            TransmitCommand => {
                if self.state == State::CommandRetryTransmit {
                    self.command = fallback_command(self.current_command);
                    self.acknowledgement = CommandWord::new(self.acknowledgement.raw() & !0x0700_0000);
                }
                self.start_word(self.command)?;
                self.previous_command = self.current_command;
                self.current_command = self.command;
            },
            ValidateCommand => {
                let allow_invalid_magic = self.previous_states[0] == State::CompletionDelay && self.previous_states[1] == State::RecoveryTransmit;
                let result = classify_command(bytes, self.previous_command, self.acknowledgement, self.recovery, allow_invalid_magic).map_err(Error::Frame)?;
                self.last_response = Some(result.response);
                self.acknowledgement = result.acknowledgement;
                self.recovery = result.recovery;
                return Ok(Self::outcome(result.outcome, result.recovery));
            },
            CommandDelay => {
                let delay = if self.previous_states[0] == State::CommandTransmit && self.command.command_id() != 0 { self.timing.command } else { self.timing.normal };
                self.arm(delay)?;
            },
            TransmitAcknowledgment => self.start_word(self.acknowledgement.with_checksum())?,
            ValidateAcknowledgment => {
                let result = classify_ack(bytes, ResponseContext {
                    current_command: self.current_command, previous_command: self.previous_command,
                    last_response: self.last_response, expected_sequence: self.expected_sequence,
                    recovery: self.recovery, busy_count: self.busy_count, busy_limit: self.timing.busy_limit,
                }).map_err(Error::Frame)?;
                self.recovery = result.recovery;
                let completed_sequence = self.expected_sequence;
                self.expected_sequence = result.expected_sequence;
                self.busy_count = result.busy_count;
                if result.reset_retry_count { self.retry_count = RetryCount::new(0); }
                if result.acknowledges_previous {
                    let slot = self.outstanding.iter().enumerate().filter_map(|(index, pending)| pending.filter(|pending| pending.sequence == completed_sequence).map(|pending| (index, pending.token.get()))).min_by_key(|(_, token)| *token).map(|(index, _)| index);
                    let delay = if let Some(slot) = slot {
                        let pending = self.outstanding[slot].take().unwrap();
                        self.completions.command_completed(pending.token, self.last_response.unwrap_or(CommandWord::new(0)))
                    } else { CallbackDelay::Normal };
                    self.delay = match delay { CallbackDelay::Normal => DelaySelection::Normal, CallbackDelay::Deferred => DelaySelection::Deferred };
                    self.recovery = match delay { CallbackDelay::Normal => RecoveryMode::Awaiting, CallbackDelay::Deferred => RecoveryMode::Normal };
                }
                return Ok(Self::outcome(result.outcome, result.recovery));
            },
            CompletionDelay => {
                let delay = match self.delay {
                    DelaySelection::Normal => self.timing.normal,
                    DelaySelection::Deferred => self.timing.deferred,
                    DelaySelection::AcceptedPage => self.timing.accepted_page,
                    DelaySelection::RejectedPage => self.timing.rejected_page,
                };
                self.arm(delay)?;
                self.delay = DelaySelection::Normal;
            },
            RetryDelay => self.arm(self.timing.command)?,
            RecoveryDelay => {
                self.retry_count = self.retry_count.increment();
                if self.timing.recovery_limit.exceeded(self.retry_count) { return Ok(Some(Event::Exhausted)); }
                self.arm(self.timing.recovery)?;
            },
            TransmitRecovery => self.start_word(CommandWord::new(0xa55a_3cc3))?,
            ValidateRecovery => if classify_reset(bytes).map_err(Error::Frame)? != ResetResponse::Accepted { return Ok(Some(Event::Recover)); },
            Fail => {
                if self.timing.failure_mode == FailureMode::DiagnosticBlink { self.backend.diagnostic_halt(); }
                self.completions.transport_failed();
                self.retire(CompletionStatus::Failed);
            },
            TransmitWrite | TransmitRead => {
                let page = self.page.ok_or(Error::Busy)?;
                let (length, extra) = match page {
                    PageRequest::Write { length, marker, .. } => (length, marker == ReadMarker::Enabled),
                    PageRequest::Read { length, option, .. } => (length, option == TransferOption::ExtraByte),
                };
                let token = TransferToken::new(self.generation()?).ok_or(Error::TokenExhausted)?;
                self.backend.start(token, &self.frame.as_bytes()[..length.get() + 2 + usize::from(extra)]).map_err(Error::Backend)?;
                self.transfer = Some(token);
            },
            CompleteWrite => {
                if let Some(PageRequest::Write { token, length, marker }) = self.page {
                    let response = classify_page_write(bytes, length, marker).map_err(Error::Frame)?;
                    self.completions.page_written(token, response);
                    self.page_delivered = true;
                } else { return Err(Error::Busy); }
            },
            CompleteRead => {
                if let Some(PageRequest::Read { token, length, option }) = self.page {
                    let data = bytes.get(..length.get() + 2 + usize::from(option == TransferOption::ExtraByte)).ok_or(Error::Frame(FrameError::ShortFrame))?;
                    let page = PageId::new(u16::from_be_bytes([data[0], data[1]]));
                    self.completions.page_read(token, page, &data[2..]);
                    self.page_delivered = true;
                } else { return Err(Error::Busy); }
            },
        }
        Ok(core::option::Option::None)
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
mod tests;

#[cfg(all(feature = "native-codec", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! { loop { core::hint::spin_loop(); } }
