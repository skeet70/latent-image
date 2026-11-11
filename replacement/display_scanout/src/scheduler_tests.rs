// Synthetic requests and service outcomes are derived from the scanout contract, not camera captures.
use super::scheduler::*;
use crate::{Hardware, Register};
use std::vec::Vec;
use core::cell::Cell;
use std::rc::Rc;

#[derive(Clone, Copy)]
struct Publication { output: Handle, phase: Phase, critical: bool }

#[derive(Debug, PartialEq)]
enum Effect { Read, Enter, Leave(u32), Configure(IrqSource, Handler), Release(Handle) }
struct Device {
    status: u32,
    effects: Vec<Effect>,
    configure_result: Result<Handle, IrqError>,
    release_result: Result<(), IrqError>,
    publication: Option<Rc<Cell<Publication>>>,
}
impl Default for Device {
    fn default() -> Self { Self { status: 2, effects: Vec::new(), configure_result: Ok(Handle(99)), release_result: Ok(()), publication: None } }
}
impl Hardware for Device {
    fn read(&mut self, register: Register) -> u32 {
        assert_eq!(register.offset, 0x30);
        self.effects.push(Effect::Read);
        self.status
    }
    fn write(&mut self, _: Register, _: u32) { panic!("scheduler writes no MMIO"); }
    fn enter_critical(&mut self) -> u32 {
        self.effects.push(Effect::Enter);
        if let Some(publication) = &self.publication { publication.set(Publication { critical: true, ..publication.get() }); }
        7
    }
    fn leave_critical(&mut self, token: u32) {
        self.effects.push(Effect::Leave(token));
        if let Some(publication) = &self.publication { publication.set(Publication { critical: false, ..publication.get() }); }
    }
    fn yield_zero(&mut self) { panic!("scheduler does not yield"); }
}
impl Irq for Device {
    fn configure(&mut self, source: IrqSource, handler: Handler) -> Result<Handle, IrqError> {
        if let Some(publication) = &self.publication {
            let published = publication.get();
            assert_eq!(published.output, Handle(1));
            assert_eq!(published.phase, Phase::One);
            assert!(published.critical);
        }
        self.effects.push(Effect::Configure(source, handler)); self.configure_result
    }
    fn release(&mut self, handle: Handle) -> Result<(), IrqError> {
        self.effects.push(Effect::Release(handle)); self.release_result
    }
}
#[derive(Default)]
struct Recorder { calls: Vec<u32>, requeue: bool, insert_persistent: bool }
impl Callbacks<Device, LocalState> for Recorder {
    fn invoke(&mut self, callback: CallbackId, scheduler: &Scheduler, hardware: &mut Device) {
        self.calls.push(callback.get());
        if callback.get() == 1 && self.requeue {
            self.requeue = false;
            register(scheduler, hardware, request(3, Retention::OneShot, 5, Phase::Zero)).unwrap();
        }
        if callback.get() == 1 && self.insert_persistent {
            self.insert_persistent = false;
            register(scheduler, hardware, request(3, Retention::PersistentActive, 30, Phase::Zero)).unwrap();
        }
    }
}
fn request(callback: u32, retention: Retention, order: u32, phase: Phase) -> Request {
    Request { callback: CallbackId::new(callback).unwrap(), retention, order: Order(order), phase }
}
fn register(scheduler: &Scheduler, hardware: &mut Device, request: Request) -> Result<Handle, RegistrationError> {
    scheduler.register(request, Handler::TimingZero, hardware)
}

#[test]
fn stable_unsigned_ties_and_high_priority_fifth_evict_tail() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    for (callback, order) in [(1,10), (2,10), (3,4), (4,2), (5,u32::MAX)] {
        assert!(register(&scheduler, &mut hardware, request(callback, Retention::PersistentActive, order, Phase::Zero)).is_ok());
    }
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [5,1,2,3]);
}

#[test]
fn zero_order_capacity_and_null_have_distinct_public_effects() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    let mut output = Handle(88);
    let null = RegistrationRequest { callback: None, retention: Retention::PersistentActive, order: Order(10), phase: Phase::Zero };
    assert_eq!(scheduler.register_observed(null, &mut output, Handler::TimingZero, &mut hardware, &mut ()), Err(RegistrationError::NullCallback));
    assert_eq!(output, Handle(0));
    assert_eq!(hardware.effects, [Effect::Enter, Effect::Leave(7)]);
    hardware.effects.clear();
    let zero = RegistrationRequest { callback: CallbackId::new(1), retention: Retention::PersistentActive, order: Order(0), phase: Phase::Zero };
    assert_eq!(scheduler.register_observed(zero, &mut output, Handler::TimingZero, &mut hardware, &mut ()), Err(RegistrationError::Capacity));
    assert_eq!(output, Handle(0));
    assert_eq!(hardware.effects, [Effect::Enter, Effect::Leave(7)]);
    assert_eq!(register(&scheduler, &mut hardware, request(2, Retention::OneShot, 1, Phase::Zero)), Ok(Handle(2)));
    for callback in 3..6 { register(&scheduler, &mut hardware, request(callback, Retention::OneShot, 1, Phase::Zero)).unwrap(); }
    assert_eq!(register(&scheduler, &mut hardware, request(6, Retention::OneShot, 1, Phase::Zero)), Err(RegistrationError::Capacity));
}

#[test]
fn callback_requeue_at_passed_slot_waits_and_shifted_later_work_runs_now() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    register(&scheduler, &mut hardware, request(1, Retention::OneShot, 20, Phase::Zero)).unwrap();
    register(&scheduler, &mut hardware, request(2, Retention::OneShot, 10, Phase::Zero)).unwrap();
    let mut callbacks = Recorder { requeue: true, ..Recorder::default() };
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [1,2]);
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [1,2,3]);
}

#[test]
fn persistent_callback_insertion_changes_live_remaining_iterations() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    register(&scheduler, &mut hardware, request(1, Retention::PersistentActive, 20, Phase::Zero)).unwrap();
    register(&scheduler, &mut hardware, request(2, Retention::PersistentActive, 10, Phase::Zero)).unwrap();
    let mut callbacks = Recorder { insert_persistent: true, ..Recorder::default() };
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [1,1,2]);
    callbacks.calls.clear();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [3,1,2]);
}

#[test]
fn selector_two_uses_persistent_list_without_initial_activity() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    register(&scheduler, &mut hardware, request(1, Retention::from_stock(2), 1, Phase::Zero)).unwrap();
    hardware.effects.clear();
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Release(Handle(99))]);
    assert_eq!(callbacks.calls, [1]);
    hardware.effects.clear();
    hardware.status = 0;
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Configure(IrqSource::Eleven, Handler::TimingZero), Effect::Leave(7)]);
    assert_eq!(callbacks.calls, [1]);
}

#[test]
fn registration_failure_keeps_inserted_work_and_does_not_set_activity() {
    let scheduler = Scheduler::default();
    let mut hardware = Device { configure_result: Err(IrqError(-9)), ..Device::default() };
    assert_eq!(register(&scheduler, &mut hardware, request(1, Retention::PersistentActive, 1, Phase::One)), Ok(Handle(1)));
    hardware.effects.clear();
    hardware.status = 0;
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Leave(7)]);
    assert_eq!(callbacks.calls, [1]);
    hardware.effects.clear();
    hardware.status = 2;
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Configure(IrqSource::Twelve, Handler::TimingZero), Effect::Leave(7)]);
}

#[test]
fn both_handlers_switch_sources_once_and_preserve_failed_release_handle() {
    for handler in [Handler::TimingZero, Handler::TimingOne] {
        let scheduler = Scheduler::default();
        let mut hardware = Device::default();
        let request = Request { callback: CallbackId::new(1).unwrap(), retention: Retention::PersistentActive, order: Order(1), phase: Phase::One };
        scheduler.register(request, handler, &mut hardware).unwrap();
        hardware.effects.clear();
        let mut callbacks = Recorder::default();
        scheduler.dispatch(handler, &mut hardware, &mut callbacks);
        let upper = if handler == Handler::TimingZero { IrqSource::Twelve } else { IrqSource::Ten };
        assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Release(Handle(99)), Effect::Configure(upper, handler), Effect::Leave(7)]);
        hardware.effects.clear();
        hardware.release_result = Err(IrqError(-1));
        hardware.status = 0;
        scheduler.dispatch(handler, &mut hardware, &mut callbacks);
        assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Release(Handle(99)), Effect::Leave(7)]);
        assert_eq!(callbacks.calls, [1]);
    }
}

#[test]
fn one_shots_precede_persistent_and_phase_count_decrements_only_on_delivery() {
    let scheduler = Scheduler::default();
    let mut hardware = Device { status: 0, ..Device::default() };
    register(&scheduler, &mut hardware, request(1, Retention::PersistentActive, 10, Phase::One)).unwrap();
    register(&scheduler, &mut hardware, request(2, Retention::OneShot, 10, Phase::One)).unwrap();
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [2,1]);
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [2,1,1]);
}

#[test]
fn counter_wrap_and_zero_repair_are_visible_in_handles() {
    let scheduler = Scheduler::default();
    scheduler.state.set_next_handle(Handle(u32::MAX));
    let mut hardware = Device::default();
    assert_eq!(register(&scheduler, &mut hardware, request(1, Retention::OneShot, 1, Phase::Zero)), Ok(Handle(u32::MAX)));
    assert_eq!(register(&scheduler, &mut hardware, request(2, Retention::OneShot, 2, Phase::Zero)), Ok(Handle(1)));
}

#[test]
fn rejected_selector_one_still_activates_source_switching() {
    let scheduler = Scheduler::default();
    let mut hardware = Device { status: 0, ..Device::default() };
    assert_eq!(register(&scheduler, &mut hardware, request(1, Retention::PersistentActive, 0, Phase::Zero)), Err(RegistrationError::Capacity));
    hardware.effects.clear();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut Recorder::default());
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Configure(IrqSource::Eleven, Handler::TimingZero), Effect::Leave(7)]);
    hardware.effects.clear();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut Recorder::default());
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Release(Handle(99)), Effect::Leave(7)]);
}

#[test]
fn eviction_does_not_repair_phase_count() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    register(&scheduler, &mut hardware, request(1, Retention::OneShot, 1, Phase::One)).unwrap();
    for callback in 2..6 { register(&scheduler, &mut hardware, request(callback, Retention::OneShot, callback, Phase::Zero)).unwrap(); }
    hardware.effects.clear();
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [5,4,3,2]);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Release(Handle(99)), Effect::Leave(7)]);
    hardware.effects.clear();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Leave(7)]);
}

#[test]
fn failed_switch_registration_clears_handle_and_next_dispatch_can_retry() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    register(&scheduler, &mut hardware, request(1, Retention::PersistentActive, 1, Phase::Zero)).unwrap();
    hardware.status = 0;
    hardware.configure_result = Err(IrqError(-1));
    hardware.effects.clear();
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Release(Handle(99)), Effect::Configure(IrqSource::Eleven, Handler::TimingZero), Effect::Leave(7)]);
    hardware.configure_result = Ok(Handle(100));
    hardware.effects.clear();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(hardware.effects, [Effect::Read, Effect::Enter, Effect::Configure(IrqSource::Eleven, Handler::TimingZero), Effect::Leave(7)]);
    hardware.status = 2;
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [1]);
}

struct Observer { publication: Rc<Cell<Publication>>, outputs: Vec<Handle>, phases: Vec<Phase> }
impl RegistrationObserver for Observer {
    fn output(&mut self, output: Handle) {
        assert!(self.publication.get().critical);
        self.publication.set(Publication { output, ..self.publication.get() });
        self.outputs.push(output);
    }
    fn phase(&mut self, phase: Phase) {
        assert!(self.publication.get().critical);
        self.publication.set(Publication { phase, ..self.publication.get() });
        self.phases.push(phase);
    }
}

#[test]
fn caller_output_and_phase_are_published_inside_critical_before_irq_configuration() {
    let scheduler = Scheduler::default();
    let publication = Rc::new(Cell::new(Publication { output: Handle(777), phase: Phase::Zero, critical: false }));
    let mut hardware = Device { publication: Some(publication.clone()), ..Device::default() };
    let mut observer = Observer { publication, outputs: Vec::new(), phases: Vec::new() };
    let mut output = Handle(777);
    let request = RegistrationRequest { callback: CallbackId::new(1), retention: Retention::PersistentActive, order: Order(1), phase: Phase::One };
    scheduler.register_observed(request, &mut output, Handler::TimingZero, &mut hardware, &mut observer).unwrap();
    assert_eq!(observer.outputs, [Handle(0), Handle(1)]);
    assert_eq!(observer.phases, [Phase::One]);
    assert!(!observer.publication.get().critical);
    observer.outputs.clear();
    observer.phases.clear();
    assert_eq!(scheduler.register_observed(RegistrationRequest { callback: None, ..request }, &mut output, Handler::TimingZero, &mut hardware, &mut observer), Err(RegistrationError::NullCallback));
    assert_eq!(observer.outputs, [Handle(0)]);
    assert!(observer.phases.is_empty());
    assert!(!observer.publication.get().critical);
}

#[test]
fn shifting_preserves_opaque_phase_selectors_without_delivering_them() {
    let scheduler = Scheduler::default();
    let mut hardware = Device::default();
    register(&scheduler, &mut hardware, request(1, Retention::PersistentActive, 10, Phase::Zero)).unwrap();
    scheduler.state.set_slot_phase(List::Persistent, SlotIndex::First, SlotPhase(0xf000_0001));
    register(&scheduler, &mut hardware, request(2, Retention::PersistentActive, 20, Phase::Zero)).unwrap();
    assert_eq!(scheduler.state.slot_phase(List::Persistent, SlotIndex::Second), SlotPhase(0xf000_0001));
    let mut callbacks = Recorder::default();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    hardware.status = 0;
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut callbacks);
    assert_eq!(callbacks.calls, [2]);
}

struct DeliveredSlotObserver;
impl Callbacks<Device, LocalState> for DeliveredSlotObserver {
    fn invoke(&mut self, _: CallbackId, scheduler: &Scheduler, _: &mut Device) {
        assert_eq!(scheduler.state.slot_callback(List::OneShot, SlotIndex::First), None);
        assert_eq!(scheduler.state.slot_order(List::OneShot, SlotIndex::First), Order(0));
        assert_eq!(scheduler.state.slot_handle(List::OneShot, SlotIndex::First), Handle(0));
        assert_eq!(scheduler.state.slot_phase(List::OneShot, SlotIndex::First), SlotPhase::from_phase(Phase::Zero));
        assert_eq!(scheduler.state.phase_count(), PhaseCount(0));
    }
}

#[test]
fn delivered_one_shot_is_cleared_before_callback_observes_state() {
    let scheduler = Scheduler::default();
    let mut hardware = Device { status: 0, ..Device::default() };
    register(&scheduler, &mut hardware, request(1, Retention::OneShot, 10, Phase::One)).unwrap();
    scheduler.dispatch(Handler::TimingZero, &mut hardware, &mut DeliveredSlotObserver);
}
