use super::*;
use crate::audio_zoom_actor::{BankPhase, Channels, Gain, InputMode, OutputMode, Rate};
use xm1_power_lifecycle::audio_release::callbacks::AudioAction;
use xm1_power_lifecycle::audio_release::executor::{Action, DelayTicks, ExecutorDiagnostic, ExecutorMode, NodeAddress, OperationHandle, OperationNode, OperationSlot, SlotState};
use xm1_power_services::primitives::ServiceStatus;

struct Memory { mode: ExecutorMode, slots: [OperationSlot; 10], generations: [OperationHandle; 10], counter: OperationHandle }
impl Memory {
    fn new() -> Self {
        Self { mode: ExecutorMode::Idle, slots: [OperationSlot { state: SlotState::Free, remaining: DelayTicks(0), node: NodeAddress(0) }; 10], generations: [OperationHandle(0); 10], counter: OperationHandle(0) }
    }
}
impl ExecutorMemory for Memory {
    fn mode(&self) -> ExecutorMode { self.mode }
    fn set_mode(&mut self, mode: ExecutorMode) { self.mode = mode; }
    fn slot(&self, index: usize) -> OperationSlot { self.slots[index] }
    fn set_slot(&mut self, index: usize, slot: OperationSlot) { self.slots[index] = slot; }
    fn generation(&self, index: usize) -> OperationHandle { self.generations[index] }
    fn set_generation(&mut self, index: usize, generation: OperationHandle) { self.generations[index] = generation; }
    fn generation_counter(&self) -> OperationHandle { self.counter }
    fn set_generation_counter(&mut self, counter: OperationHandle) { self.counter = counter; }
    fn node(&self, _: NodeAddress) -> OperationNode {
        OperationNode { delay: DelayTicks(2), action: Action::OutputOff, next: None }
    }
}
#[derive(Default)]
struct Device { running: bool, output_enabled: bool, fail_start: bool, bank_notices: usize }
impl ExecutorIo for Device {
    type SavedIrq = ();
    fn save_irq(&mut self) {}
    fn restore_irq(&mut self, _: ()) {}
    fn start_tick_timer(&mut self) -> ServiceStatus {
        if self.fail_start { ServiceStatus(-7) } else { self.running = true; ServiceStatus(0) }
    }
    fn stop_tick_timer(&mut self) -> ServiceStatus { self.running = false; ServiceStatus(0) }
    fn diagnostic(&mut self, _: ExecutorDiagnostic) {}
    fn invoke(&mut self, action: Action) -> Result<(), Action> {
        match action {
            Action::OutputOff => { self.output_enabled = false; Ok(()) },
            Action::Audio(AudioAction::BankNotify | AudioAction::BankNotifyUnchanged) => { self.bank_notices += 1; Ok(()) },
            _ => Err(action),
        }
    }
}
struct Worker { bank: BankSelection, phase: BankPhase }
impl Worker {
    fn idle() -> Self { Self { bank: BankSelection::Primary, phase: BankPhase::Idle } }
}
impl WorkerFields for Worker {
    fn initialize(&mut self) {}
    fn configure_input(&mut self, _: InputMode, _: Rate) {}
    fn configure_output(&mut self, _: OutputMode, _: Gain, _: Channels, _: Rate) {}
    fn alternate_bank(&self) -> bool { self.bank == BankSelection::Alternate }
    fn scheduled(&mut self, request: Request) {
        if !matches!(request, Request::SelectBank { .. }) { self.phase = BankPhase::Unavailable; }
    }
    fn completed(&mut self, request: Request) {
        match request {
            Request::Release => self.phase = BankPhase::Released,
            Request::Input { operation: crate::audio_zoom_actor::Operation::Start, .. } => self.phase = BankPhase::Active,
            Request::SelectBank { .. } => {}
            _ => self.phase = BankPhase::Idle,
        }
    }
    fn bank_phase(&self) -> BankPhase { self.phase }
    fn select_bank(&mut self, bank: BankSelection, transition: bool) {
        self.bank = bank;
        if transition { self.phase = BankPhase::Unavailable; }
    }
}
fn timeout() -> Timeout { Timeout::new(20).unwrap() }
fn initialize(actor: &mut AudioZoomActor, memory: &mut Memory, io: &mut Device, worker: &mut Worker) {
    actor.submit(memory, io, worker, Request::Initialize, Tick(0), timeout()).unwrap();
    actor.drive(memory, io, worker, Tick(1)).unwrap();
    actor.drive(memory, io, worker, Tick(2)).unwrap();
}

#[test]
fn every_nonzero_selector_selects_the_alternate_bank_without_starting_idle_hardware() {
    for selector in 0..=u8::MAX {
        let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
        assert_eq!(runtime.begin(CaptureMode::from_selector(selector), &mut actor, &mut memory, &mut io, &mut worker, Tick(0), timeout()), Ok(Progress::Completed(Completion::Complete)));
        assert_eq!(worker.alternate_bank(), selector != 0);
        assert!(!io.running);
        assert_eq!(io.bank_notices, 1);
    }
}

#[test]
fn active_bank_change_waits_for_executor_effect_and_poll_does_not_advance_it() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    initialize(&mut actor, &mut memory, &mut io, &mut worker);
    worker.phase = BankPhase::Active;
    io.output_enabled = true;
    assert_eq!(runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(2), timeout()), Ok(Progress::Waiting));
    for _ in 0..4 { assert_eq!(runtime.poll(&actor), Ok(Progress::Waiting)); }
    assert!(io.output_enabled);
    assert_eq!(runtime.begin(CaptureMode::Default, &mut actor, &mut memory, &mut io, &mut worker, Tick(2), timeout()), Err(Error::Busy));
    actor.drive(&mut memory, &mut io, &mut worker, Tick(3)).unwrap();
    assert_eq!(runtime.poll(&actor), Ok(Progress::Waiting));
    actor.drive(&mut memory, &mut io, &mut worker, Tick(4)).unwrap();
    assert!(!io.output_enabled);
    assert_eq!(runtime.poll(&actor), Ok(Progress::Completed(Completion::Complete)));
}

#[test]
fn selecting_the_current_active_bank_requires_no_hardware_transition() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    initialize(&mut actor, &mut memory, &mut io, &mut worker);
    worker.phase = BankPhase::Active;
    io.fail_start = true;
    assert_eq!(runtime.begin(CaptureMode::Default, &mut actor, &mut memory, &mut io, &mut worker, Tick(2), timeout()), Ok(Progress::Completed(Completion::Complete)));
    assert_eq!(io.bank_notices, 1);
}

#[test]
fn cancellation_prevents_late_hardware_effects_and_never_becomes_ready() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    initialize(&mut actor, &mut memory, &mut io, &mut worker);
    worker.phase = BankPhase::Active;
    io.output_enabled = true;
    runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(2), timeout()).unwrap();
    assert_eq!(runtime.cancel(&mut actor, &mut memory, &mut io), Ok(Progress::Completed(Completion::Cancelled)));
    actor.drive(&mut memory, &mut io, &mut worker, Tick(3)).unwrap();
    actor.drive(&mut memory, &mut io, &mut worker, Tick(4)).unwrap();
    assert!(io.output_enabled);
    assert_eq!(runtime.poll(&actor), Ok(Progress::Completed(Completion::Cancelled)));
}

#[test]
fn expired_transition_is_not_capture_admission() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    initialize(&mut actor, &mut memory, &mut io, &mut worker);
    worker.phase = BankPhase::Active;
    io.output_enabled = true;
    runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(u32::MAX), Timeout::new(1).unwrap()).unwrap();
    actor.drive(&mut memory, &mut io, &mut worker, Tick(0)).unwrap();
    assert_eq!(runtime.poll(&actor), Ok(Progress::Completed(Completion::TimedOut)));
    assert!(io.output_enabled);
}

#[test]
fn deferred_request_runs_only_after_worker_admission() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    worker.phase = BankPhase::Unavailable;
    assert_eq!(runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(0), timeout()), Ok(Progress::Waiting));
    assert_eq!(runtime.resume(&mut actor, &mut memory, &mut io, &mut worker, Tick(1)), Ok(Progress::Waiting));
    assert!(!worker.alternate_bank());
    worker.phase = BankPhase::Idle;
    assert_eq!(runtime.resume(&mut actor, &mut memory, &mut io, &mut worker, Tick(2)), Ok(Progress::Completed(Completion::Complete)));
    assert!(worker.alternate_bank());
}

#[test]
fn cancelling_deferred_request_prevents_later_admission() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    worker.phase = BankPhase::Unavailable;
    runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(0), timeout()).unwrap();
    assert_eq!(runtime.cancel(&mut actor, &mut memory, &mut io), Ok(Progress::Completed(Completion::Cancelled)));
    worker.phase = BankPhase::Idle;
    assert_eq!(runtime.resume(&mut actor, &mut memory, &mut io, &mut worker, Tick(1)), Ok(Progress::Completed(Completion::Cancelled)));
    assert!(!worker.alternate_bank());
}

#[test]
fn released_worker_rejects_instead_of_deferring() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    worker.phase = BankPhase::Released;
    assert_eq!(runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(0), timeout()), Err(Error::Codec(CodecError::NotInitialized)));
    assert_eq!(runtime.poll(&actor), Ok(Progress::Completed(Completion::Failed(CodecError::NotInitialized))));
}

#[test]
fn driver_failure_is_terminal_not_capture_admission() {
    let (mut runtime, mut actor, mut memory, mut io, mut worker) = (AvConfigurationRuntime::new(), AudioZoomActor::new(), Memory::new(), Device::default(), Worker::idle());
    initialize(&mut actor, &mut memory, &mut io, &mut worker);
    worker.phase = BankPhase::Active;
    io.fail_start = true;
    assert_eq!(runtime.begin(CaptureMode::Alternate, &mut actor, &mut memory, &mut io, &mut worker, Tick(2), timeout()), Err(Error::Codec(CodecError::Driver(-7))));
    assert_eq!(runtime.poll(&actor), Ok(Progress::Completed(Completion::Failed(CodecError::Driver(-7)))));
}
