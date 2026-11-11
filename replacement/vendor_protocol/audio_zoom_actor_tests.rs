use super::*;
use xm1_power_lifecycle::audio_release::{callbacks::AudioAction, executor::{DelayTicks, ExecutorDiagnostic, ExecutorMode, OperationNode, OperationSlot, SlotState}};
use xm1_power_services::primitives::ServiceStatus;

struct Memory { mode: ExecutorMode, slots: [OperationSlot; 10], generations: [OperationHandle; 10], counter: OperationHandle }
impl Memory {
    fn new() -> Self { Self { mode: ExecutorMode::Idle, slots: [OperationSlot { state: SlotState::Free, remaining: DelayTicks(0), node: NodeAddress(0) }; 10], generations: [OperationHandle(0); 10], counter: OperationHandle(0) } }
}
impl ExecutorMemory for Memory {
    fn mode(&self) -> ExecutorMode { self.mode }
    fn set_mode(&mut self, mode: ExecutorMode) { self.mode = mode; }
    fn slot(&self, index: usize) -> OperationSlot { self.slots[index] }
    fn set_slot(&mut self, index: usize, slot: OperationSlot) { self.slots[index] = slot; }
    fn generation(&self, index: usize) -> OperationHandle { self.generations[index] }
    fn set_generation(&mut self, index: usize, value: OperationHandle) { self.generations[index] = value; }
    fn generation_counter(&self) -> OperationHandle { self.counter }
    fn set_generation_counter(&mut self, value: OperationHandle) { self.counter = value; }
    fn node(&self, address: NodeAddress) -> OperationNode {
        let action = match address.0 {
            0x8291cc => Action::Audio(AudioAction::Enable13),
            0x7d1c90 | 0x7d1b58 => Action::Audio(AudioAction::ClockOutput),
            0x7d1454 | 0x7d1e14 | 0x7d15b0 => Action::Audio(AudioAction::ClockInput),
            0x7d1e80 | 0x7d1640 | 0x7d1b1c | 0x7d1b34 => Action::OutputOff,
            _ => Action::Other(address.0),
        };
        OperationNode { delay: DelayTicks(2), action, next: None }
    }
}
#[derive(Default)]
struct Device { enabled: bool, input_clock: bool, output_clock: bool, timer_running: bool, fail_action: bool, fail_timer: bool, bank_notifications: u32 }
impl ExecutorIo for Device {
    type SavedIrq = ();
    fn save_irq(&mut self) {}
    fn restore_irq(&mut self, _: ()) {}
    fn start_tick_timer(&mut self) -> ServiceStatus { if self.fail_timer { ServiceStatus(-7) } else { self.timer_running = true; ServiceStatus(0) } }
    fn stop_tick_timer(&mut self) -> ServiceStatus { self.timer_running = false; ServiceStatus(0) }
    fn diagnostic(&mut self, _: ExecutorDiagnostic) {}
    fn invoke(&mut self, action: Action) -> Result<(), Action> {
        if self.fail_action { return Err(action); }
        match action {
            Action::Audio(AudioAction::Enable13) => self.enabled = true,
            Action::Audio(AudioAction::ClockInput) => self.input_clock = true,
            Action::Audio(AudioAction::ClockOutput) => self.output_clock = true,
            Action::OutputOff => { self.input_clock = false; self.output_clock = false; },
            Action::Audio(AudioAction::BankNotify | AudioAction::BankNotifyUnchanged) => self.bank_notifications += 1,
            _ => return Err(action),
        }
        Ok(())
    }
}
#[derive(Default)]
struct Worker { input: Option<(InputMode, Rate)>, output: Option<(OutputMode, Gain, Channels, Rate)>, alternate: bool, active: bool }
impl WorkerFields for Worker {
    fn initialize(&mut self) { self.input = None; self.output = None; }
    fn configure_input(&mut self, mode: InputMode, rate: Rate) { self.input = Some((mode, rate)); }
    fn configure_output(&mut self, mode: OutputMode, gain: Gain, channels: Channels, rate: Rate) { self.output = Some((mode, gain, channels, rate)); }
    fn alternate_bank(&self) -> bool { self.alternate }
    fn bank_phase(&self) -> BankPhase { if self.active { BankPhase::Active } else { BankPhase::Idle } }
    fn select_bank(&mut self, bank: BankSelection, _: bool) { self.alternate = bank == BankSelection::Alternate; }
    fn scheduled(&mut self, _: Request) {}
    fn completed(&mut self, request: Request) { self.active = matches!(request, Request::Input { operation: Operation::Start, .. }); }
}
fn timeout() -> Timeout { Timeout::new(20).unwrap() }
fn initialize(actor: &mut AudioZoomActor, memory: &mut Memory, device: &mut Device, worker: &mut Worker) {
    let id = actor.submit(memory, device, worker, Request::Initialize, Tick(0), timeout()).unwrap();
    actor.drive(memory, device, worker, Tick(1)).unwrap();
    assert_eq!(actor.result(id), Ok(None));
    actor.drive(memory, device, worker, Tick(2)).unwrap();
    assert_eq!(actor.result(id), Ok(Some(Completion::Complete)));
    assert!(device.enabled);
}
fn capture(operation: Operation) -> Request { Request::Input { operation, mode: InputMode::Format8, rate: Rate::Hz48000 } }

#[test]
fn capture_completes_only_after_codec_programming_and_stop_removes_clock() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    assert_eq!(actor.submit(&mut memory, &mut device, &mut worker, capture(Operation::Start), Tick(0), timeout()), Err(Error::NotInitialized));
    initialize(&mut actor, &mut memory, &mut device, &mut worker);
    let id = actor.submit(&mut memory, &mut device, &mut worker, capture(Operation::Start), Tick(2), timeout()).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(3)).unwrap();
    assert!(!device.output_clock);
    assert_eq!(actor.result(id), Ok(None));
    actor.drive(&mut memory, &mut device, &mut worker, Tick(4)).unwrap();
    assert!(device.output_clock);
    assert_eq!(worker.input, Some((InputMode::Format8, Rate::Hz48000)));
    assert_eq!(actor.result(id), Ok(Some(Completion::Complete)));
    let stop = actor.submit(&mut memory, &mut device, &mut worker, capture(Operation::Stop), Tick(4), timeout()).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(5)).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(6)).unwrap();
    assert_eq!(actor.result(stop), Ok(None));
    actor.drive(&mut memory, &mut device, &mut worker, Tick(7)).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(8)).unwrap();
    assert_eq!(actor.result(stop), Ok(Some(Completion::Complete)));
    assert!(!device.output_clock);
}

#[test]
fn playback_zero_gain_is_valid_and_busy_request_cannot_replace_it() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    initialize(&mut actor, &mut memory, &mut device, &mut worker);
    let request = Request::Output { operation: Operation::Start, mode: OutputMode::Playback, gain: Gain::new(0).unwrap(), channels: Channels::Secondary, rate: Rate::Hz48000 };
    let id = actor.submit(&mut memory, &mut device, &mut worker, request, Tick(2), timeout()).unwrap();
    assert_eq!(actor.submit(&mut memory, &mut device, &mut worker, Request::Release, Tick(2), timeout()), Err(Error::Busy));
    actor.drive(&mut memory, &mut device, &mut worker, Tick(3)).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(4)).unwrap();
    assert!(device.input_clock);
    assert_eq!(actor.result(id), Ok(Some(Completion::Complete)));
}

#[test]
fn cancel_prevents_late_programming_and_cannot_complete_a_new_request() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    initialize(&mut actor, &mut memory, &mut device, &mut worker);
    let old = actor.submit(&mut memory, &mut device, &mut worker, capture(Operation::Start), Tick(2), timeout()).unwrap();
    actor.cancel(&mut memory, &mut device, old).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(3)).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(4)).unwrap();
    assert!(!device.output_clock);
    assert_eq!(actor.result(old), Ok(Some(Completion::Cancelled)));
    let fresh = actor.submit(&mut memory, &mut device, &mut worker, Request::Initialize, Tick(4), timeout()).unwrap();
    assert_eq!(actor.result(old), Err(Error::WrongRequest));
    assert_eq!(actor.cancel(&mut memory, &mut device, old), Err(Error::WrongRequest));
    assert_eq!(actor.result(fresh), Ok(None));
}

#[test]
fn timeout_across_clock_wrap_cancels_instead_of_succeeding() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    let id = actor.submit(&mut memory, &mut device, &mut worker, Request::Initialize, Tick(u32::MAX), Timeout::new(2).unwrap()).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(0)).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(1)).unwrap();
    assert_eq!(actor.result(id), Ok(Some(Completion::TimedOut)));
    assert!(!device.enabled);
    assert!(!device.timer_running);
    actor.drive(&mut memory, &mut device, &mut worker, Tick(2)).unwrap();
    assert_eq!(actor.result(id), Ok(Some(Completion::TimedOut)));
}

#[test]
fn driver_and_action_errors_do_not_report_completion() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    device.fail_timer = true;
    assert_eq!(actor.submit(&mut memory, &mut device, &mut worker, Request::Initialize, Tick(0), timeout()), Err(Error::Driver(-7)));
    device.fail_timer = false;
    device.fail_action = true;
    let id = actor.submit(&mut memory, &mut device, &mut worker, Request::Initialize, Tick(1), timeout()).unwrap();
    actor.drive(&mut memory, &mut device, &mut worker, Tick(2)).unwrap();
    assert!(matches!(actor.drive(&mut memory, &mut device, &mut worker, Tick(3)), Err(Error::Action(_))));
    assert!(matches!(actor.result(id), Ok(Some(Completion::Failed(Error::Action(_))))));
    assert!(!device.enabled);
}

#[test]
fn selecting_a_bank_while_idle_publishes_without_starting_the_executor_timer() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    let id = actor.submit(&mut memory, &mut device, &mut worker, Request::SelectBank { bank: BankSelection::Alternate }, Tick(0), timeout()).unwrap();
    assert_eq!(actor.result(id), Ok(Some(Completion::Complete)));
    assert!(worker.alternate_bank());
    assert_eq!(device.bank_notifications, 1);
    assert!(!device.timer_running);
}

#[test]
fn observing_completion_does_not_tick_the_shared_executor() {
    let (mut actor, mut memory, mut device, mut worker) = (AudioZoomActor::new(), Memory::new(), Device::default(), Worker::default());
    let id = actor.submit(&mut memory, &mut device, &mut worker, Request::Initialize, Tick(0), timeout()).unwrap();
    actor.observe(&mut memory, &mut device, &mut worker, Tick(1)).unwrap();
    actor.observe(&mut memory, &mut device, &mut worker, Tick(2)).unwrap();
    assert_eq!(actor.result(id), Ok(None));
    assert!(!device.enabled);
    executor::tick(&mut memory, &mut device).unwrap();
    executor::tick(&mut memory, &mut device).unwrap();
    actor.observe(&mut memory, &mut device, &mut worker, Tick(3)).unwrap();
    assert_eq!(actor.result(id), Ok(Some(Completion::Complete)));
    assert!(device.enabled);
}
