use super::*;
use crate::primitives::{PoolBlock, QueueDiagnostic, QueueId, ReceivePoll, StorageSlot};
use std::cell::Cell;
use std::rc::Rc;
use std::vec::Vec;

struct State { phase: AudioPhase, completion: Option<CompletionAddress> }
impl State {
    fn ready() -> Self { Self { phase: AudioPhase::from_abi(2), completion: None } }
}
impl AudioState for State {
    fn phase(&self) -> AudioPhase { self.phase }
    fn set_phase(&mut self, phase: AudioPhase) { self.phase = phase; }
    fn completion(&self) -> Option<CompletionAddress> { self.completion }
    fn set_completion(&mut self, completion: Option<CompletionAddress>) { self.completion = completion; }
}
#[derive(Default)]
struct Queue { messages: Vec<ReleaseMessage>, status: i32 }
impl AudioQueue for Queue {
    fn send(&mut self, message: ReleaseMessage) -> ServiceStatus {
        if self.status == 0 { self.messages.push(message); }
        ServiceStatus(self.status)
    }
}
struct Device {
    power: CodecWord,
    output: CodecWord,
    writes: Vec<CodecWord>,
    open_status: i32,
    configure_status: i32,
    write_status: i32,
    pending_polls: usize,
}
impl Default for Device {
    fn default() -> Self {
        Self { power: CodecWord(0xffff), output: CodecWord(0xffff), writes: Vec::new(), open_status: 0, configure_status: 0, write_status: 0, pending_polls: 0 }
    }
}
impl AudioDevice for Device {
    fn shadow(&self, register: CodecRegister) -> CodecWord { match register { CodecRegister::Power => self.power, CodecRegister::Output => self.output } }
    fn store_shadow(&mut self, register: CodecRegister, value: CodecWord) { match register { CodecRegister::Power => self.power = value, CodecRegister::Output => self.output = value } }
    fn open(&mut self) -> Result<SerialHandle, ServiceStatus> {
        if self.open_status == 0 { Ok(SerialHandle(3)) } else { Err(ServiceStatus(self.open_status)) }
    }
    fn configure(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(self.configure_status) }
    fn write(&mut self, _: SerialHandle, value: CodecWord) -> ServiceStatus {
        if self.write_status == 0 { self.writes.push(value); }
        ServiceStatus(self.write_status)
    }
    fn status(&mut self, _: SerialHandle, status: &mut CodecStatus) -> ServiceStatus {
        if self.pending_polls == 0 { status.receive = 1; status.transmit = 1; }
        else { self.pending_polls -= 1; status.receive = 1; status.transmit = 0; }
        ServiceStatus(0)
    }
    fn stop(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn close(&mut self, _: SerialHandle) -> ServiceStatus { ServiceStatus(0) }
    fn lock_codec(&mut self) -> ServiceStatus { ServiceStatus(0) }
    fn unlock_codec(&mut self) -> ServiceStatus { ServiceStatus(0) }
}
struct Rtos { queue_deleted: Rc<Cell<bool>>, remaining: usize }
impl QueueRtos for Rtos {
    fn receive_poll(&mut self, _: QueueId) -> ReceivePoll {
        if self.remaining == 0 { ReceivePoll::Stopped(ServiceStatus(-50)) }
        else { self.remaining -= 1; ReceivePoll::Message(PoolBlock(0x40001000)) }
    }
    fn release_block(&mut self, _: QueueId, _: PoolBlock) -> ServiceStatus { ServiceStatus(0) }
    fn delete_pool(&mut self, _: QueueId) -> ServiceStatus { ServiceStatus(0) }
    fn delete_queue(&mut self, _: QueueId) -> ServiceStatus { self.queue_deleted.set(true); ServiceStatus(0) }
}
struct Storage;
impl QueueStorage for Storage {
    fn release_storage(&mut self, _: StorageSlot) -> ServiceStatus { ServiceStatus(0) }
    fn clear_active(&mut self, _: QueueId) {}
    fn diagnostic(&mut self, _: QueueDiagnostic, _: u32, _: ServiceStatus) {}
    fn record_error(&mut self, _: ServiceStatus) {}
}
struct Callbacks { queue_deleted: Rc<Cell<bool>>, signaled: bool, retired: bool }
impl AudioCallbacks for Callbacks {
    fn invoke(&mut self, completion: CompletionAddress) {
        assert_eq!(completion.address(), 0xec5660);
        assert!(self.queue_deleted.get());
        self.signaled = true;
    }
    fn retire_task41(&mut self) -> ServiceStatus { self.retired = true; ServiceStatus(0) }
}
fn fixture() -> (State, Queue, Device, Callbacks, Rtos, Storage) {
    let deleted = Rc::new(Cell::new(false));
    (State::ready(), Queue::default(), Device::default(), Callbacks { queue_deleted: deleted.clone(), signaled: false, retired: false }, Rtos { queue_deleted: deleted, remaining: 2 }, Storage)
}

#[test]
fn request_only_posts_and_never_completes_shutdown() {
    let (state, mut queue, _, callbacks, rtos, _) = fixture();
    assert!(request_release(&state, &mut queue, CompletionAddress::from_abi(0xec5660)).is_ok());
    assert_eq!(queue.messages, [ReleaseMessage { command: ReleaseCommand::Request, completion: CompletionAddress::from_abi(0xec5660) }]);
    assert!(!callbacks.signaled);
    assert!(!rtos.queue_deleted.get());
    assert_eq!(state.phase().word(), 2);
}

#[test]
fn readiness_and_transport_failures_return_minus255() {
    let (mut state, mut queue, _, _, _, _) = fixture();
    state.set_phase(AudioPhase::from_abi(0));
    assert_eq!(request_release(&state, &mut queue, None), Err(ServiceStatus(-255)));
    assert!(queue.messages.is_empty());
    state.set_phase(AudioPhase::from_abi(8));
    queue.status = -62;
    assert_eq!(request_release(&state, &mut queue, None), Err(ServiceStatus(-255)));
    assert!(queue.messages.is_empty());
}

use executor::{Action, DelayTicks, ExecutorDiagnostic, ExecutorIo, ExecutorMemory, ExecutorMode, NodeAddress, OperationHandle, OperationNode, OperationSlot, SlotState};

struct Memory {
    mode: ExecutorMode,
    slots: [OperationSlot; 10],
    handles: [OperationHandle; 10],
    counter: OperationHandle,
}
impl Memory {
    fn ready() -> Self {
        Self {
            mode: ExecutorMode::Idle,
            slots: [OperationSlot { state: SlotState::Free, remaining: DelayTicks(0), node: NodeAddress(0) }; 10],
            handles: [OperationHandle(0); 10], counter: OperationHandle(0),
        }
    }
}
impl ExecutorMemory for Memory {
    fn mode(&self) -> ExecutorMode { self.mode }
    fn set_mode(&mut self, mode: ExecutorMode) { self.mode = mode; }
    fn slot(&self, index: usize) -> OperationSlot { self.slots[index] }
    fn set_slot(&mut self, index: usize, slot: OperationSlot) { self.slots[index] = slot; }
    fn generation(&self, index: usize) -> OperationHandle { self.handles[index] }
    fn set_generation(&mut self, index: usize, generation: OperationHandle) { self.handles[index] = generation; }
    fn generation_counter(&self) -> OperationHandle { self.counter }
    fn set_generation_counter(&mut self, value: OperationHandle) { self.counter = value; }
    fn node(&self, address: NodeAddress) -> OperationNode {
        match address.0 {
            0x7d1c28 => OperationNode { delay: DelayTicks(1), action: Action::Noop, next: Some(NodeAddress(0x7d1c34)) },
            0x7d1c34 => OperationNode { delay: DelayTicks(1), action: Action::OutputOff, next: Some(NodeAddress(0x7d1c40)) },
            0x7d1c40 => OperationNode { delay: DelayTicks(1), action: Action::PowerOff, next: Some(NodeAddress(0x7d1c4c)) },
            0x7d1c4c => OperationNode { delay: DelayTicks(1), action: Action::PostFinished, next: None },
            other => panic!("unmapped node {other:x}"),
        }
    }
}
struct Io<'a> { device: &'a mut Device, queue: &'a mut Queue, armed: bool }
impl ExecutorIo for Io<'_> {
    type SavedIrq = ();
    fn save_irq(&mut self) {}
    fn restore_irq(&mut self, _: ()) {}
    fn start_tick_timer(&mut self) -> ServiceStatus { self.armed = true; ServiceStatus(0) }
    fn stop_tick_timer(&mut self) -> ServiceStatus { self.armed = false; ServiceStatus(0) }
    fn invoke(&mut self, action: Action) -> Result<(), Action> { run_release_action(self.device, self.queue, action) }
    fn diagnostic(&mut self, _: ExecutorDiagnostic) {}
}
struct Operations<'a, 'd> { memory: &'a mut Memory, io: &'a mut Io<'d>, handle: OperationHandle }
impl AudioOperations for Operations<'_, '_> {
    fn schedule_release(&mut self) -> ServiceStatus { executor::schedule(self.memory, self.io, NodeAddress(0x7d1c28), &mut self.handle) }
}
struct ScheduleError;
impl AudioOperations for ScheduleError {
    fn schedule_release(&mut self) -> ServiceStatus { ServiceStatus(-20) }
}

#[test]
fn release_runs_on_four_worker_ticks_then_completes_only_on_task41_receive() {
    let (mut state, mut queue, mut device, mut callbacks, mut rtos, mut storage) = fixture();
    device.pending_polls = 3;
    request_release(&state, &mut queue, CompletionAddress::from_abi(0xec5660)).unwrap();
    let request = queue.messages.remove(0);
    let mut memory = Memory::ready();
    let mut io = Io { device: &mut device, queue: &mut queue, armed: false };
    {
        let mut operations = Operations { memory: &mut memory, io: &mut io, handle: OperationHandle(0) };
        assert_eq!(consume_release(&mut state, &mut operations, &mut callbacks, &mut rtos, &mut storage, request), ReleaseDispatch::SequenceScheduled(ServiceStatus(0)));
        assert_ne!(operations.handle, OperationHandle(0));
    }
    assert_eq!(state.phase().word(), 6);
    assert!(io.armed);
    assert!(io.device.writes.is_empty());
    executor::tick(&mut memory, &mut io).unwrap();
    assert!(io.device.writes.is_empty());
    executor::tick(&mut memory, &mut io).unwrap();
    assert_eq!(io.device.writes, [CodecWord(0xfff3)]);
    executor::tick(&mut memory, &mut io).unwrap();
    assert_eq!(io.device.writes, [CodecWord(0xfff3), CodecWord(0xffdf)]);
    assert!(io.queue.messages.is_empty());
    executor::tick(&mut memory, &mut io).unwrap();
    assert!(!io.armed);
    assert!(!callbacks.signaled);
    let completion = io.queue.messages.remove(0);
    assert_eq!(completion.command, ReleaseCommand::SequenceFinished);
    assert_eq!(consume_release(&mut state, &mut ScheduleError, &mut callbacks, &mut rtos, &mut storage, completion), ReleaseDispatch::TaskRetired);
    assert!(callbacks.signaled);
    assert!(callbacks.retired);
    assert_eq!(state.phase().word(), 0);
    assert_eq!(state.completion(), None);
}

#[test]
fn scheduler_capacity_and_generation_wrap_are_caller_visible() {
    let mut memory = Memory::ready();
    memory.counter = OperationHandle(u32::MAX);
    let mut device = Device::default();
    let mut queue = Queue::default();
    let mut io = Io { device: &mut device, queue: &mut queue, armed: false };
    let mut returned = Vec::new();
    for _ in 0..10 {
        let mut handle = OperationHandle(0);
        assert_eq!(executor::schedule(&mut memory, &mut io, NodeAddress(0x7d1c28), &mut handle), ServiceStatus(0));
        assert_ne!(handle, OperationHandle(0));
        assert!(!returned.contains(&handle));
        returned.push(handle);
    }
    let mut untouched = OperationHandle(99);
    assert_eq!(executor::schedule(&mut memory, &mut io, NodeAddress(0x7d1c28), &mut untouched), ServiceStatus(-20));
    assert_eq!(untouched, OperationHandle(99));
    for _ in 0..4 { executor::tick(&mut memory, &mut io).unwrap(); }
    assert_eq!(io.queue.messages.len(), 10);
    assert_eq!(executor::schedule(&mut memory, &mut io, NodeAddress(0x7d1c28), &mut untouched), ServiceStatus(0));
}

#[test]
fn schedule_failure_does_not_signal_or_destroy() {
    let (mut state, _, _, mut callbacks, mut rtos, mut storage) = fixture();
    let request = ReleaseMessage { command: ReleaseCommand::Request, completion: CompletionAddress::from_abi(0xec5660) };
    assert_eq!(consume_release(&mut state, &mut ScheduleError, &mut callbacks, &mut rtos, &mut storage, request), ReleaseDispatch::SequenceScheduled(ServiceStatus(-20)));
    assert_eq!(state.phase().word(), 6);
    assert!(!callbacks.signaled);
    assert!(!rtos.queue_deleted.get());
}

#[test]
fn stale_finished_message_does_not_complete_an_unstarted_release() {
    let (mut state, _, _, mut callbacks, mut rtos, mut storage) = fixture();
    let message = ReleaseMessage { command: ReleaseCommand::SequenceFinished, completion: None };
    assert_eq!(consume_release(&mut state, &mut ScheduleError, &mut callbacks, &mut rtos, &mut storage, message), ReleaseDispatch::NotSelected);
    assert!(!callbacks.signaled);
    assert!(!rtos.queue_deleted.get());
}

#[test]
fn malformed_release_payloads_do_not_enter_the_consumer() {
    assert_eq!(ReleaseMessage::decode(0x3900, &[0; 3]), Err(ServiceStatus(-255)));
    assert_eq!(ReleaseMessage::decode(0x3906, &[0; 5]), Err(ServiceStatus(-255)));
    assert_eq!(ReleaseMessage::decode(0x3901, &[]), Ok(None));
}

#[test]
fn absent_callback_still_retires_task() {
    let (mut state, _, _, mut callbacks, mut rtos, mut storage) = fixture();
    state.set_phase(AudioPhase::from_abi(6));
    let message = ReleaseMessage { command: ReleaseCommand::SequenceFinished, completion: None };
    assert_eq!(consume_release(&mut state, &mut ScheduleError, &mut callbacks, &mut rtos, &mut storage, message), ReleaseDispatch::TaskRetired);
    assert!(callbacks.retired);
    assert!(!callbacks.signaled);
    assert_eq!(state.phase().word(), 0);
}

#[test]
fn cancelled_generation_cannot_cancel_a_reused_slot_and_late_tick_is_inert() {
    let mut memory = Memory::ready();
    let mut device = Device::default();
    let mut queue = Queue::default();
    let mut io = Io { device: &mut device, queue: &mut queue, armed: false };
    let mut first = OperationHandle(0);
    assert_eq!(executor::schedule(&mut memory, &mut io, NodeAddress(0x7d1c28), &mut first), ServiceStatus(0));
    assert_eq!(executor::current_node(&memory, &mut io, first), Ok(NodeAddress(0x7d1c28)));
    assert_eq!(executor::cancel(&mut memory, &mut io, first), ServiceStatus(0));
    executor::tick(&mut memory, &mut io).unwrap();
    assert!(io.device.writes.is_empty());
    assert!(io.queue.messages.is_empty());
    let mut second = OperationHandle(0);
    assert_eq!(executor::schedule(&mut memory, &mut io, NodeAddress(0x7d1c28), &mut second), ServiceStatus(0));
    assert_ne!(first, second);
    assert_eq!(executor::cancel(&mut memory, &mut io, first), ServiceStatus(-30));
    for _ in 0..4 { executor::tick(&mut memory, &mut io).unwrap(); }
    assert_eq!(io.queue.messages.len(), 1);
    assert_eq!(executor::current_node(&memory, &mut io, second), Err(ServiceStatus(-30)));
}

#[test]
fn shutdown_callback_sets_task_two_flag_one_and_propagates_service_failure() {
    use crate::shutdown::{KernelStatus, ServiceMessage, ShutdownRtos, TaskId};
    struct Flags { events: Vec<(u32, u32)>, status: i32 }
    impl ShutdownRtos for Flags {
        fn current_task(&mut self) -> TaskId { TaskId(41) }
        fn post(&mut self, _: ServiceMessage<'_>) -> Result<(), KernelStatus> { panic!("unexpected message") }
        fn set_event_flag(&mut self, flag: u32, bits: u32) -> Result<(), KernelStatus> {
            self.events.push((flag, bits));
            if self.status == 0 { Ok(()) } else { Err(KernelStatus(self.status)) }
        }
    }
    let mut flags = Flags { events: Vec::new(), status: 0 };
    assert_eq!(signal_shutdown_complete(&mut flags), ServiceStatus(0));
    assert_eq!(flags.events, [(2, 1)]);
    flags.status = -42;
    assert_eq!(signal_shutdown_complete(&mut flags), ServiceStatus(-42));
}

#[test]
fn codec_edits_preserve_register_tags_and_bits_outside_the_selected_field() {
    use callbacks::{edit_value, CacheBits, CacheKeep};
    assert_eq!(edit_value(0x12ff, CacheKeep(0xfff3), CacheBits(0)), 0x12f3);
    assert_eq!(edit_value(0x55fe, CacheKeep(0xff00), CacheBits(0x1b)), 0x551b);
    assert_eq!(edit_value(0x8c00, CacheKeep(0xffff), CacheBits(0x20)), 0x8c20);
}

unsafe extern "C" fn audio_stream_receive() {}
unsafe extern "C" fn audio_stream_send() {}
unsafe extern "C" fn audio_operation_tick() {}

#[test]
fn stream_completion_registers_the_handler_the_timer_entry_resolves() {
    use crate::callback_registry::{registry, CallbackFn};
    for handler in [audio_stream_receive as unsafe extern "C" fn(), audio_stream_send as unsafe extern "C" fn()] {
        let word = stream_pipeline::register_completion(handler).unwrap();
        assert_eq!(word, handler.word());
        assert_eq!(registry().resolve::<unsafe extern "C" fn()>(word).map(|callback| callback.word()), Ok(word));
    }
}

#[test]
fn executor_tick_registers_before_the_timer_entry_resolves_it() {
    use crate::callback_registry::{registry, CallbackFn};
    let handler = audio_operation_tick as unsafe extern "C" fn();
    crate::task6_executor::register_executor_tick(handler).unwrap();
    let word = handler.word();
    assert_eq!(registry().resolve::<unsafe extern "C" fn()>(word).map(|callback| callback.word()), Ok(word));
}

#[test]
fn executor_tick_refusal_is_returned() {
    use crate::callback_registry::CallbackError;
    let null: unsafe extern "C" fn() = unsafe { core::mem::transmute(0usize) };
    assert_eq!(crate::task6_executor::register_executor_tick(null), Err(CallbackError::Invalid));
}

#[test]
fn capture_gpio_recipe_restores_output_modes_and_drives_both_pins_high() {
    use capture_callbacks::{CaptureAction, CaptureCompletion, CaptureEvent, CaptureField, CaptureGpio, CaptureHardware, ClockSourceState, FieldValue, GpioLevel};
    struct Pins { levels: [bool; 34], modes: [u8; 34], events: Vec<CaptureEvent> }
    impl CaptureHardware for Pins {
        fn complete(&mut self, _: CaptureCompletion) -> Result<(), u32> { Ok(()) }
        fn gpio_level(&mut self, pin: CaptureGpio, level: GpioLevel) { self.levels[pin.0 as usize] = level == GpioLevel::High; }
        fn gpio_mode_zero(&mut self, pin: CaptureGpio) { self.modes[pin.0 as usize] = 0; }
        fn gpio_mode_one(&mut self, pin: CaptureGpio) { self.modes[pin.0 as usize] = 1; }
        fn clock_mode(&mut self) -> u8 { 4 }
        fn select_clock_source(&mut self, _: ClockSourceState) {}
        fn set_clock_divider(&mut self, _: u32) {}
        fn acquire_clock_resource(&mut self) -> ServiceStatus { ServiceStatus(0) }
        fn release_clock_resource(&mut self) -> ServiceStatus { ServiceStatus(0) }
        fn release_field_value(&self, _: CaptureField) -> FieldValue { FieldValue(0) }
        fn request_field(&mut self, _: CaptureField, _: FieldValue) {}
        fn post_event(&mut self, event: CaptureEvent) { self.events.push(event); }
    }
    let mut pins = Pins { levels: [false; 34], modes: [0; 34], events: Vec::new() };
    capture_callbacks::invoke(&mut pins, CaptureAction::EnableSixAndSeven).unwrap();
    assert!(pins.levels[6] && pins.levels[7]);
    assert_eq!((pins.modes[6], pins.modes[7]), (1, 1));
    capture_callbacks::invoke(&mut pins, CaptureAction::Notify17).unwrap();
    assert_eq!(pins.events, [CaptureEvent::Completed]);
}
