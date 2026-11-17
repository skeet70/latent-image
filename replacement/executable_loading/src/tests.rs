use super::*;
use crate::client::{ClientError, CommandCompletion, CompletionWords};
use crate::protocol::*;
use crate::receiver::Receiver;
use crate::sender::Sender;
use std::{cell::Cell, collections::VecDeque, rc::Rc, vec::Vec};
use xm1_application_nand::{PhysicalBlock, BLOCK_BYTES};
use xm1_application_nand::dma_read::{
    BlockCount, BlockDisposition, ByteCount, CleanupError, DmaDestination, DmaReadOwner, LowerStatus,
    Phase, PrimaryError, ReadOutcome, ReadPlan, ReadPorts, SkipBudget,
};

#[derive(Clone)]
struct Counter(Rc<Cell<u64>>);
impl Clock for Counter {
    fn now(&mut self) -> Instant { Instant::new(self.0.get()) }
}
fn report() -> ReadReport { ReadReport { bytes: ByteCount::new(4096), bad_blocks: BlockCount::new(2).unwrap() } }
fn failure() -> Failure {
    Failure::Primary { primary: PrimaryError::Lower(LowerStatus::new(-17).unwrap()),
        cleanup: Some(CleanupError::Deadline(Phase::Quiescence)) }
}
struct Engine { outcome: Option<ProducerCompletion>, counter: Counter, finish_at: u64, resource_generation: Rc<Cell<u32>> }
impl Producer for Engine {
    fn execute(&mut self, _: Request) -> ProducerCompletion {
        self.resource_generation.set(self.resource_generation.get() + 1);
        self.counter.0.set(self.finish_at);
        self.outcome.take().expect("unexpected executable reuse")
    }
}
struct Sink { events: Vec<(Request, Publication)>, counter: Counter, expire_on_commit: bool }
impl ReceiverKernel for Sink {
    fn publish(&mut self, request: Request, publication: Publication) -> Result<(), ProtocolError> {
        let successful = matches!(publication.completion.outcome, CompletionOutcome::Loaded(_) | CompletionOutcome::Delegated);
        if self.expire_on_commit && successful {
            self.counter.0.set(request.deadline.instant().count());
            self.expire_on_commit = false;
        }
        if successful && !request.live(self.counter.now()) { return Err(ProtocolError::Expired); }
        self.events.push((request, publication));
        Ok(())
    }
}
struct Transport<P = Engine> {
    queue: VecDeque<(u16, [u8; ENVELOPE_BYTES])>,
    completions: VecDeque<Completion>,
    receiver: Receiver,
    engine: P,
    sink: Sink,
    counter: Counter,
    capacity: Option<(u32, u32, u32)>,
    spurious: bool,
}
impl Transport<Engine> {
    fn new(outcome: ProducerCompletion) -> Self {
        let counter = Counter(Rc::new(Cell::new(10)));
        Self { queue: VecDeque::new(), completions: VecDeque::new(), receiver: Receiver::new(),
            engine: Engine { outcome: Some(outcome), counter: counter.clone(), finish_at: 11, resource_generation: Rc::new(Cell::new(0)) },
            sink: Sink { events: Vec::new(), counter: counter.clone(), expire_on_commit: false },
            counter, capacity: None, spurious: false }
    }
}
impl<P: Producer> Transport<P> {
    fn deliver(&mut self) -> Result<Completion, ProtocolError> {
        let (operation, envelope) = self.queue.pop_front().unwrap();
        let completion = self.receiver.receive(operation, &envelope, &mut self.counter, &mut self.engine, &mut self.sink)?;
        self.completions.push_back(completion);
        Ok(completion)
    }
}
impl<P: Producer> SenderKernel for Transport<P> {
    fn initialize_queue(&mut self, queue: u32, capacity_bytes: u32, depth: u32) -> Result<(), ProtocolError> {
        self.capacity = Some((queue, capacity_bytes, depth)); Ok(())
    }
    fn start_receiver(&mut self) -> Result<(), ProtocolError> { Ok(()) }
    fn post(&mut self, _: u32, operation: u16, envelope: &[u8; ENVELOPE_BYTES], deadline: xm1_application_nand::dma_read::Deadline) -> Result<(), ProtocolError> {
        if self.counter.now().count() >= deadline.instant().count() { return Err(ProtocolError::Expired); }
        self.queue.push_back((operation, *envelope)); Ok(())
    }
    fn take_completion(&mut self, id: RequestId, kind: CommandKind) -> Option<Completion> {
        let index = self.completions.iter().position(|completion| completion.id == id && completion.kind == kind)?;
        self.completions.remove(index)
    }
    fn wait(&mut self, _: RequestId, _: CommandKind, deadline: xm1_application_nand::dma_read::Deadline) -> Result<(), ProtocolError> {
        if self.spurious { self.spurious = false; return Ok(()); }
        if self.queue.is_empty() { self.counter.0.set(deadline.instant().count()); return Err(ProtocolError::Expired); }
        self.deliver().map(|_| ())
    }
}
fn request_deadline() -> xm1_application_nand::dma_read::Deadline {
    xm1_application_nand::dma_read::Deadline::new(Instant::new(30)).unwrap()
}
fn loaded() -> ProducerCompletion { ProducerCompletion::Loaded(unsafe { LoadedProof::from_validated_read(report()) }) }

#[test]
fn sync_consumer_receives_loaded_proof_after_spurious_wake() {
    let mut transport = Transport::new(loaded());
    transport.spurious = true;
    let mut clock = transport.counter.clone();
    let mut sender = Sender::new(&mut transport).unwrap();
    let completion = sender.request_sync(CommandKind::Plain, request_deadline(), &mut clock, &mut transport).unwrap();
    assert_eq!(CompletionWords::from_completion(completion).unwrap().parse(CommandKind::Plain),
        Ok(CommandCompletion::Loaded { request_id: completion.id, kind: CommandKind::Plain,
            completed_bytes: ByteCount::new(4096), bad_blocks: BlockCount::new(2).unwrap() }));
    assert_eq!(transport.capacity, Some((60, 48, 16)));
    assert!(transport.sink.events[0].1.completed_latch);
    assert!(!transport.receiver.busy());
}

#[test]
fn wire_preserves_original_deadline_callback_and_argument() {
    let mut transport = Transport::new(loaded());
    let mut clock = transport.counter.clone();
    let mut sender = Sender::new(&mut transport).unwrap();
    let callback = Callback::Address(CallbackAddress::new(0x8000).unwrap());
    let request = sender.send(CommandKind::Plain, request_deadline(), callback, CallbackArgument(0xabcdef01), &mut clock, &mut transport).unwrap();
    let (operation, bytes) = transport.queue.front().unwrap();
    assert_eq!(*operation, 0x4c03);
    assert_eq!(bytes.len(), 24);
    assert_eq!(Request::decode(*operation, bytes).unwrap(), request);
    assert_eq!(&bytes[8..16], &30u64.to_le_bytes());
    assert_eq!(&bytes[16..20], &0x8000u32.to_le_bytes());
    assert_eq!(&bytes[20..24], &0xabcdef01u32.to_le_bytes());
    transport.deliver().unwrap();
    assert_eq!(transport.sink.events[0].0.callback, callback);
    assert_eq!(transport.sink.events[0].0.argument, CallbackArgument(0xabcdef01));
}

#[test]
fn startup_batch_keeps_distinct_request_deadlines_and_completions() {
    let mut transport = Transport::new(ProducerCompletion::Delegated);
    let mut clock = transport.counter.clone();
    let mut sender = Sender::new(&mut transport).unwrap();
    let kinds = [CommandKind::FixedBlock, CommandKind::Selected, CommandKind::Region, CommandKind::Section3,
        CommandKind::Initialize, CommandKind::RegisterCompletion, CommandKind::Auxiliary];
    let mut requests = Vec::new();
    for kind in kinds { requests.push(sender.send(kind, request_deadline(), Callback::None, CallbackArgument(kind as u32), &mut clock, &mut transport).unwrap()); }
    assert_eq!(transport.queue.len(), 7);
    for request in requests {
        transport.engine.outcome = Some(ProducerCompletion::Delegated);
        transport.deliver().unwrap();
        let completion = sender.wait(request, &mut clock, &mut transport).unwrap();
        assert_eq!(CompletionWords::from_completion(completion).unwrap().parse(request.kind),
            Ok(CommandCompletion::Delegated { request_id: request.id, kind: request.kind }));
    }
    assert!(transport.sink.events.iter().all(|(_, publication)| publication.completed_latch));
    assert!(transport.sink.events.iter().all(|(_, publication)| publication.callback_result.legacy_failed() == 0));
}

#[test]
fn lower_failure_retains_primary_cleanup_and_quarantine() {
    for retention in [Retention::Released, Retention::HeldQuiescent, Retention::Unproved] {
        let mut transport = Transport::new(ProducerCompletion::Failed { failure: failure(), report: report(), retention });
        let mut clock = transport.counter.clone();
        let mut sender = Sender::new(&mut transport).unwrap();
        let completion = sender.request_sync(CommandKind::Plain, request_deadline(), &mut clock, &mut transport).unwrap();
        assert_eq!(CompletionWords::from_completion(completion).unwrap().parse(CommandKind::Plain),
            Err(ClientError::Failure { request_id: completion.id, kind: CommandKind::Plain, failure: failure(), report: report(), retention }));
        assert!(!transport.sink.events[0].1.completed_latch);
        assert_eq!(transport.sink.events[0].1.callback_result.legacy_failed(), 1);
        assert_eq!(transport.receiver.busy(), retention != Retention::Released);
        if retention != Retention::Released {
            assert_eq!(sender.send(CommandKind::Image5, request_deadline(), Callback::None, CallbackArgument(0), &mut clock, &mut transport), Err(ProtocolError::RetainedOperation));
            let mut next = transport.sink.events[0].0;
            next.id = next.id.next().unwrap();
            assert_eq!(transport.receiver.receive(next.kind.operation(), &next.encode(), &mut clock, &mut transport.engine, &mut transport.sink), Err(ProtocolError::RetainedOperation));
        }
    }
}

#[test]
fn expiry_before_or_at_commit_never_publishes_success() {
    for atomic_expiry in [false, true] {
        let mut transport = Transport::new(loaded());
        transport.engine.finish_at = if atomic_expiry { 11 } else { 30 };
        transport.sink.expire_on_commit = atomic_expiry;
        let mut clock = transport.counter.clone();
        let mut sender = Sender::new(&mut transport).unwrap();
        sender.send(CommandKind::Plain, request_deadline(), Callback::None, CallbackArgument(0), &mut clock, &mut transport).unwrap();
        let completion = transport.deliver().unwrap();
        assert!(matches!(completion.outcome, CompletionOutcome::Failed { retention: Retention::Released, .. }));
        assert_eq!(transport.sink.events.len(), 1);
        assert!(!transport.sink.events[0].1.completed_latch);
    }
}

#[test]
fn legacy_and_out_of_bounds_envelopes_are_refused() {
    let mut transport = Transport::new(loaded());
    let mut clock = transport.counter.clone();
    assert_eq!(transport.receiver.receive(0x4c03, &[0; 8], &mut clock, &mut transport.engine, &mut transport.sink), Err(ProtocolError::EnvelopeSize));
    assert_eq!(transport.receiver.receive(0x4c09, &[0; 24], &mut clock, &mut transport.engine, &mut transport.sink), Err(ProtocolError::InvalidCommand));
    assert!(transport.sink.events.is_empty());
}

#[test]
fn mismatched_completion_does_not_turn_a_wake_into_success() {
    let mut transport = Transport::new(loaded());
    let mut clock = transport.counter.clone();
    let mut sender = Sender::new(&mut transport).unwrap();
    let request = sender.send(CommandKind::Plain, request_deadline(), Callback::None, CallbackArgument(0), &mut clock, &mut transport).unwrap();
    transport.queue.clear();
    transport.completions.push_back(Completion { id: request.id.next().unwrap(), kind: request.kind, outcome: CompletionOutcome::Loaded(report()) });
    assert_eq!(sender.wait(request, &mut clock, &mut transport), Err(ProtocolError::Expired));
    assert_eq!(sender.pending_for_kind(CommandKind::Plain), Some(request));
    assert_eq!(sender.send(CommandKind::Image5, request_deadline(), Callback::None, CallbackArgument(0), &mut clock, &mut transport),
        Err(ProtocolError::RetainedOperation));
}

#[test]
fn malformed_completion_words_cannot_claim_loaded_residency() {
    let completion = Completion { id: RequestId::new(1).unwrap(), kind: CommandKind::Plain, outcome: CompletionOutcome::Loaded(report()) };
    let words = CompletionWords::from_completion(completion).unwrap();
    for field in 0..5 {
        let mut malformed = words;
        match field { 0 => malformed.tag = 5, 1 => malformed.request_id = 0, 2 => malformed.command_kind = 9,
            3 => malformed.protocol_code = 1, _ => malformed.primary_code = 4 }
        assert!(malformed.parse(CommandKind::Plain).is_err());
    }
    assert!(words.parse(CommandKind::Image5).is_err());
}

#[test]
fn duplicate_requests_reuse_only_owned_published_residency() {
    for kind in [CommandKind::Plain, CommandKind::Initialize, CommandKind::RegisterCompletion] {
        let mut transport = Transport::new(if kind.owns_executable() { loaded() } else { ProducerCompletion::Delegated });
        let generation = transport.engine.resource_generation.clone();
        let mut clock = transport.counter.clone();
        let mut sender = Sender::new(&mut transport).unwrap();
        let first = sender.request_sync(kind, request_deadline(), &mut clock, &mut transport).unwrap();
        assert_eq!(generation.get(), 1);
        let second = sender.request_sync(kind, request_deadline(), &mut clock, &mut transport).unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(first.outcome, second.outcome);
        assert_eq!(generation.get(), 1);
        transport.receiver = Receiver::new();
        transport.engine.outcome = Some(if kind.owns_executable() { loaded() } else { ProducerCompletion::Delegated });
        sender.request_sync(kind, request_deadline(), &mut clock, &mut transport).unwrap();
        assert_eq!(generation.get(), 2);
    }
}

#[test]
fn every_failure_phase_survives_the_completion_wire() {
    for phase in [
        Phase::DriverAcquire, Phase::ChannelAcquire, Phase::Classify, Phase::Start,
        Phase::Controller, Phase::Stop, Phase::Quiescence, Phase::FinishTransfer,
        Phase::ChannelRelease, Phase::DriverRelease, Phase::SkipResource,
        Phase::SkipProgress, Phase::Decoder, Phase::Publication,
    ] {
        for failure in [
            Failure::Primary { primary: PrimaryError::Deadline(phase), cleanup: None },
            Failure::Cleanup(CleanupError::Lower { phase, status: LowerStatus::new(i32::MIN).unwrap() }),
            Failure::Primary { primary: PrimaryError::Lower(LowerStatus::new(i32::MAX).unwrap()), cleanup: Some(CleanupError::Deadline(phase)) },
        ] {
            let id = RequestId::new(7).unwrap();
            let words = CompletionWords::from_completion(Completion { id, kind: CommandKind::Image5,
                outcome: CompletionOutcome::Failed { failure, report: report(), retention: Retention::Unproved } }).unwrap();
            assert_eq!(words.parse(CommandKind::Image5), Err(ClientError::Failure {
                request_id: id, kind: CommandKind::Image5, failure, report: report(), retention: Retention::Unproved }));
        }
    }
}

#[test]
fn released_failure_does_not_cache_success_or_prevent_retry() {
    let mut transport = Transport::new(ProducerCompletion::Failed { failure: failure(), report: report(), retention: Retention::Released });
    let generation = transport.engine.resource_generation.clone();
    let mut clock = transport.counter.clone();
    let mut sender = Sender::new(&mut transport).unwrap();
    let failed = sender.request_sync(CommandKind::Plain, request_deadline(), &mut clock, &mut transport).unwrap();
    assert!(CompletionWords::from_completion(failed).unwrap().parse(CommandKind::Plain).is_err());
    transport.engine.outcome = Some(loaded());
    let retry = sender.request_sync(CommandKind::Plain, request_deadline(), &mut clock, &mut transport).unwrap();
    assert!(matches!(CompletionWords::from_completion(retry).unwrap().parse(CommandKind::Plain), Ok(CommandCompletion::Loaded { .. })));
    assert_eq!(generation.get(), 2);
}

#[test]
fn executed_skip_publishes_and_caches_command_completion_without_claiming_loaded_bytes() {
    let mut transport = Transport::new(ProducerCompletion::Skipped(report()));
    let generation = transport.engine.resource_generation.clone();
    let mut clock = transport.counter.clone();
    let mut sender = Sender::new(&mut transport).unwrap();
    let first = sender.request_sync(CommandKind::Plain, request_deadline(), &mut clock, &mut transport).unwrap();
    assert_eq!(
        CompletionWords::from_completion(first).unwrap().parse(CommandKind::Plain),
        Ok(CommandCompletion::Skipped {
            request_id: first.id,
            kind: CommandKind::Plain,
            completed_bytes: report().bytes,
            bad_blocks: report().bad_blocks,
        }),
    );
    assert!(transport.sink.events[0].1.completed_latch);
    assert_eq!(transport.sink.events[0].1.callback_result, CallbackResult::Succeeded);
    let second = sender.request_sync(CommandKind::Plain, request_deadline(), &mut clock, &mut transport).unwrap();
    assert_eq!(second.outcome, first.outcome);
    assert_eq!(generation.get(), 1);
}

struct FixedBuffer(std::boxed::Box<[u8]>);
impl AsMut<[u8]> for FixedBuffer { fn as_mut(&mut self) -> &mut [u8] { &mut self.0 } }
unsafe impl DmaDestination for FixedBuffer {}

struct QualifiedPorts { count: u64 }
unsafe impl ReadPorts for QualifiedPorts {
    type Driver = ();
    type Channel = ();
    fn now(&mut self) -> Instant { let value = self.count; self.count += 1; Instant::new(value) }
    fn try_driver(&mut self) -> core::task::Poll<Result<(), LowerStatus>> { core::task::Poll::Ready(Ok(())) }
    fn try_channel(&mut self, _: &mut ()) -> core::task::Poll<Result<(), LowerStatus>> { core::task::Poll::Ready(Ok(())) }
    fn classify(&mut self, block: PhysicalBlock) -> Result<BlockDisposition, LowerStatus> {
        Ok(if block.index() == 0 { BlockDisposition::Bad } else { BlockDisposition::Usable })
    }
    fn start(&mut self, _: &mut (), block: PhysicalBlock, destination: &mut [u8]) -> Result<(), LowerStatus> {
        destination.fill(block.index() as u8);
        Ok(())
    }
    fn poll_controller(&mut self, _: &mut ()) -> core::task::Poll<Result<(), LowerStatus>> { core::task::Poll::Ready(Ok(())) }
    fn request_stop(&mut self, _: &mut ()) -> Result<(), LowerStatus> { Ok(()) }
    fn poll_quiescent(&mut self, _: &mut ()) -> core::task::Poll<Result<(), LowerStatus>> { core::task::Poll::Ready(Ok(())) }
    fn finish_transfer(&mut self, _: &mut ()) -> Result<(), LowerStatus> { Ok(()) }
    fn close_channel(&mut self, _: &mut ()) -> Result<(), LowerStatus> { Ok(()) }
    fn unlock_driver(&mut self, _: &mut ()) -> Result<(), LowerStatus> { Ok(()) }
    fn progress(&mut self, _: ByteCount) -> Result<(), LowerStatus> { Ok(()) }
}

struct DmaEngine { owner: DmaReadOwner<QualifiedPorts, FixedBuffer> }
impl Producer for DmaEngine {
    fn execute(&mut self, request: Request) -> ProducerCompletion {
        let plan = ReadPlan::new(PhysicalBlock::new(0).unwrap(), BlockCount::new(1).unwrap(), SkipBudget::Unlimited).unwrap();
        let destination = FixedBuffer(std::vec![0u8; BLOCK_BYTES as usize].into_boxed_slice());
        match self.owner.read(plan, destination, request.deadline) {
            ReadOutcome::Success { report, .. } => ProducerCompletion::Loaded(unsafe { LoadedProof::from_validated_read(report) }),
            ReadOutcome::ReleasedFailure { report, failure, .. } =>
                ProducerCompletion::Failed { failure, report, retention: Retention::Released },
            ReadOutcome::HeldQuiescent { report, failure } =>
                ProducerCompletion::Failed { failure, report, retention: Retention::HeldQuiescent },
            ReadOutcome::UnprovedDma { report, failure } =>
                ProducerCompletion::Failed { failure, report, retention: Retention::Unproved },
        }
    }
}

#[test]
fn actual_sender_receiver_dma_owner_skips_bad_block_before_publication() {
    let counter = Counter(Rc::new(Cell::new(10)));
    let engine = DmaEngine { owner: DmaReadOwner::new(QualifiedPorts { count: 10 }) };
    let mut transport = Transport {
        queue: VecDeque::new(), completions: VecDeque::new(), receiver: Receiver::new(), engine,
        sink: Sink { events: Vec::new(), counter: counter.clone(), expire_on_commit: false },
        counter: counter.clone(), capacity: None, spurious: false,
    };
    let mut sender = Sender::new(&mut transport).unwrap();
    let completion = sender.request_sync(CommandKind::Plain, request_deadline(), &mut counter.clone(), &mut transport).unwrap();
    assert_eq!(
        CompletionWords::from_completion(completion).unwrap().parse(CommandKind::Plain),
        Ok(CommandCompletion::Loaded {
            request_id: completion.id, kind: CommandKind::Plain,
            completed_bytes: ByteCount::new(BLOCK_BYTES as usize),
            bad_blocks: BlockCount::new(1).unwrap(),
        }),
    );
    assert!(transport.sink.events[0].1.completed_latch);
    assert_eq!(transport.capacity, Some((QUEUE_ID, QUEUE_CAPACITY_BYTES, QUEUE_DEPTH)));
}
