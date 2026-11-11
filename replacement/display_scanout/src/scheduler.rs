use core::cell::Cell;
use core::num::NonZeroU32;

use crate::{Bank, Engine, Hardware, Register};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CallbackId(NonZeroU32);
impl CallbackId {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) { Some(value) => Some(Self(value)), None => None }
    }
    pub const fn get(self) -> u32 { self.0.get() }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Order(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase { Zero, One }
impl Phase {
    pub const fn from_stock(value: u32) -> Self { if value == 1 { Self::One } else { Self::Zero } }
    pub const fn word(self) -> u32 { match self { Self::Zero => 0, Self::One => 1 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlotPhase(pub u32);
impl SlotPhase {
    pub const fn from_phase(phase: Phase) -> Self { Self(phase.word()) }
    pub fn matches(self, phase: Phase) -> bool { self == Self::from_phase(phase) }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PassiveSelector(u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Retention { OneShot, PersistentActive, PersistentPassive(PassiveSelector) }
impl Retention {
    pub const fn from_stock(value: u32) -> Self {
        match value {
            0 => Self::OneShot,
            1 => Self::PersistentActive,
            _ => Self::PersistentPassive(PassiveSelector(value)),
        }
    }
    pub const fn word(self) -> u32 {
        match self { Self::OneShot => 0, Self::PersistentActive => 1, Self::PersistentPassive(value) => value.0 }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Handler { TimingZero, TimingOne }
impl Handler {
    pub const fn from_selector(value: u32) -> Self { if value == 0 { Self::TimingZero } else { Self::TimingOne } }
    fn engine(self) -> Engine { match self { Self::TimingZero => Engine::Zero, Self::TimingOne => Engine::One } }
    fn source(self, upper: bool) -> IrqSource {
        match (self, upper) {
            (Self::TimingZero, false) => IrqSource::Eleven,
            (Self::TimingZero, true) => IrqSource::Twelve,
            (Self::TimingOne, false) => IrqSource::Nine,
            (Self::TimingOne, true) => IrqSource::Ten,
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum IrqSource { Nine = 9, Ten = 10, Eleven = 11, Twelve = 12 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IrqError(pub i32);
pub trait Irq: Hardware {
    fn configure(&mut self, source: IrqSource, handler: Handler) -> Result<Handle, IrqError>;
    fn release(&mut self, handle: Handle) -> Result<(), IrqError>;
}
pub trait Callbacks<H: Irq, S: SchedulerState> {
    fn invoke(&mut self, callback: CallbackId, scheduler: &Scheduler<S>, hardware: &mut H);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum List { Persistent, OneShot }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotIndex { First, Second, Third, Fourth }
impl SlotIndex {
    pub const ALL: [Self; 4] = [Self::First, Self::Second, Self::Third, Self::Fourth];
    pub const fn index(self) -> usize { match self { Self::First => 0, Self::Second => 1, Self::Third => 2, Self::Fourth => 3 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Activity { Inactive, Active }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhaseCount(pub u32);

pub trait SchedulerState {
    fn irq_handle(&self, handler: Handler) -> Handle;
    fn set_irq_handle(&self, handler: Handler, value: Handle);
    fn next_handle(&self) -> Handle;
    fn set_next_handle(&self, value: Handle);
    fn activity(&self) -> Activity;
    fn set_activity(&self, value: Activity);
    fn phase_count(&self) -> PhaseCount;
    fn set_phase_count(&self, value: PhaseCount);
    fn slot_handle(&self, list: List, slot: SlotIndex) -> Handle;
    fn set_slot_handle(&self, list: List, slot: SlotIndex, value: Handle);
    fn slot_order(&self, list: List, slot: SlotIndex) -> Order;
    fn set_slot_order(&self, list: List, slot: SlotIndex, value: Order);
    fn slot_callback(&self, list: List, slot: SlotIndex) -> Option<CallbackId>;
    fn set_slot_callback(&self, list: List, slot: SlotIndex, value: Option<CallbackId>);
    fn slot_phase(&self, list: List, slot: SlotIndex) -> SlotPhase;
    fn set_slot_phase(&self, list: List, slot: SlotIndex, value: SlotPhase);
}

struct LocalSlot {
    handle: Cell<Handle>,
    order: Cell<Order>,
    callback: Cell<Option<CallbackId>>,
    phase: Cell<SlotPhase>,
}
impl LocalSlot {
    const fn zero() -> Self {
        Self { handle: Cell::new(Handle(0)), order: Cell::new(Order(0)), callback: Cell::new(None), phase: Cell::new(SlotPhase(0)) }
    }
}
pub struct LocalState {
    timing_zero: Cell<Handle>,
    timing_one: Cell<Handle>,
    next_handle: Cell<Handle>,
    activity: Cell<Activity>,
    phase_count: Cell<PhaseCount>,
    persistent: [LocalSlot; 4],
    one_shot: [LocalSlot; 4],
}
impl LocalState {
    pub const fn zero() -> Self {
        Self {
            timing_zero: Cell::new(Handle(0)), timing_one: Cell::new(Handle(0)),
            next_handle: Cell::new(Handle(0)), activity: Cell::new(Activity::Inactive), phase_count: Cell::new(PhaseCount(0)),
            persistent: [const { LocalSlot::zero() }; 4], one_shot: [const { LocalSlot::zero() }; 4],
        }
    }
    fn slot(&self, list: List, slot: SlotIndex) -> &LocalSlot {
        match list { List::Persistent => &self.persistent[slot.index()], List::OneShot => &self.one_shot[slot.index()] }
    }
    fn irq(&self, handler: Handler) -> &Cell<Handle> {
        match handler { Handler::TimingZero => &self.timing_zero, Handler::TimingOne => &self.timing_one }
    }
}
impl Default for LocalState { fn default() -> Self { Self::zero() } }
impl SchedulerState for LocalState {
    fn irq_handle(&self, handler: Handler) -> Handle { self.irq(handler).get() }
    fn set_irq_handle(&self, handler: Handler, value: Handle) { self.irq(handler).set(value); }
    fn next_handle(&self) -> Handle { self.next_handle.get() }
    fn set_next_handle(&self, value: Handle) { self.next_handle.set(value); }
    fn activity(&self) -> Activity { self.activity.get() }
    fn set_activity(&self, value: Activity) { self.activity.set(value); }
    fn phase_count(&self) -> PhaseCount { self.phase_count.get() }
    fn set_phase_count(&self, value: PhaseCount) { self.phase_count.set(value); }
    fn slot_handle(&self, list: List, slot: SlotIndex) -> Handle { self.slot(list, slot).handle.get() }
    fn set_slot_handle(&self, list: List, slot: SlotIndex, value: Handle) { self.slot(list, slot).handle.set(value); }
    fn slot_order(&self, list: List, slot: SlotIndex) -> Order { self.slot(list, slot).order.get() }
    fn set_slot_order(&self, list: List, slot: SlotIndex, value: Order) { self.slot(list, slot).order.set(value); }
    fn slot_callback(&self, list: List, slot: SlotIndex) -> Option<CallbackId> { self.slot(list, slot).callback.get() }
    fn set_slot_callback(&self, list: List, slot: SlotIndex, value: Option<CallbackId>) { self.slot(list, slot).callback.set(value); }
    fn slot_phase(&self, list: List, slot: SlotIndex) -> SlotPhase { self.slot(list, slot).phase.get() }
    fn set_slot_phase(&self, list: List, slot: SlotIndex, value: SlotPhase) { self.slot(list, slot).phase.set(value); }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Request {
    pub callback: CallbackId,
    pub retention: Retention,
    pub order: Order,
    pub phase: Phase,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RegistrationRequest {
    pub callback: Option<CallbackId>,
    pub retention: Retention,
    pub order: Order,
    pub phase: Phase,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegistrationError { NullCallback, Capacity }
impl RegistrationError {
    pub const fn stock_code(self) -> i32 { match self { Self::NullCallback => -30, Self::Capacity => -20 } }
}

pub trait RegistrationObserver {
    fn output(&mut self, handle: Handle);
    fn phase(&mut self, phase: Phase);
}
impl RegistrationObserver for () {
    fn output(&mut self, _: Handle) {}
    fn phase(&mut self, _: Phase) {}
}

pub struct Scheduler<S: SchedulerState = LocalState> { pub state: S }
impl Default for Scheduler<LocalState> { fn default() -> Self { Self::new(LocalState::zero()) } }
impl<S: SchedulerState> Scheduler<S> {
    pub const fn new(state: S) -> Self { Self { state } }

    pub fn register<H: Irq>(&self, request: Request, handler: Handler, hardware: &mut H) -> Result<Handle, RegistrationError> {
        let request = RegistrationRequest { callback: Some(request.callback), retention: request.retention, order: request.order, phase: request.phase };
        let mut handle = Handle(0);
        self.register_observed(request, &mut handle, handler, hardware, &mut ())?;
        Ok(handle)
    }

    pub fn register_observed<H: Irq, O: RegistrationObserver>(&self, request: RegistrationRequest, output: &mut Handle, handler: Handler, hardware: &mut H, observer: &mut O) -> Result<(), RegistrationError> {
        self.register_source_observed(request, output, || handler, hardware, observer)
    }

    pub fn register_source_observed<H: Irq, O: RegistrationObserver>(&self, request: RegistrationRequest, output: &mut Handle, select: impl FnOnce() -> Handler, hardware: &mut H, observer: &mut O) -> Result<(), RegistrationError> {
        let token = hardware.enter_critical();
        *output = Handle(0);
        observer.output(*output);
        let Some(callback) = request.callback else {
            hardware.leave_critical(token);
            return Err(RegistrationError::NullCallback);
        };
        if self.state.next_handle() == Handle(0) { self.state.set_next_handle(Handle(1)); }
        observer.phase(request.phase);
        let list = if request.retention == Retention::OneShot { List::OneShot } else { List::Persistent };
        let insertion = SlotIndex::ALL.into_iter().find(|&slot| self.state.slot_order(list, slot).0 < request.order.0);
        let result = match insertion {
            Some(slot) => {
                *output = self.state.next_handle();
                observer.output(*output);
                for destination in ((slot.index() + 1)..SlotIndex::ALL.len()).rev() {
                    let source = SlotIndex::ALL[destination - 1];
                    let destination = SlotIndex::ALL[destination];
                    self.state.set_slot_phase(list, destination, self.state.slot_phase(list, source));
                    self.state.set_slot_handle(list, destination, self.state.slot_handle(list, source));
                    self.state.set_slot_order(list, destination, self.state.slot_order(list, source));
                    self.state.set_slot_callback(list, destination, self.state.slot_callback(list, source));
                }
                self.state.set_slot_phase(list, slot, SlotPhase::from_phase(request.phase));
                self.state.set_slot_handle(list, slot, *output);
                self.state.set_slot_order(list, slot, request.order);
                self.state.set_slot_callback(list, slot, Some(callback));
                if request.phase == Phase::One { self.state.set_phase_count(PhaseCount(self.state.phase_count().0.wrapping_add(1))); }
                Ok(())
            }
            None => Err(RegistrationError::Capacity),
        };
        if result.is_ok() {
            let handler = select();
            if self.state.irq_handle(handler) == Handle(0) {
                match hardware.configure(handler.source(request.phase == Phase::One), handler) {
                    Ok(handle) => {
                        self.state.set_irq_handle(handler, handle);
                        if request.retention == Retention::PersistentActive { self.state.set_activity(Activity::Active); }
                    }
                    Err(_) => self.state.set_irq_handle(handler, Handle(0)),
                }
            } else if request.retention == Retention::PersistentActive {
                self.state.set_activity(Activity::Active);
            }
        } else if request.retention == Retention::PersistentActive {
            self.state.set_activity(Activity::Active);
        }
        self.state.set_next_handle(Handle(self.state.next_handle().0.wrapping_add(1)));
        hardware.leave_critical(token);
        result
    }

    fn release<H: Irq>(&self, handler: Handler, hardware: &mut H) {
        let handle = self.state.irq_handle(handler);
        if handle != Handle(0) && hardware.release(handle).is_ok() { self.state.set_irq_handle(handler, Handle(0)); }
    }

    pub fn dispatch<H: Irq, C: Callbacks<H, S>>(&self, handler: Handler, hardware: &mut H, callbacks: &mut C) {
        let status = hardware.read(Register { engine: handler.engine(), bank: Bank::Timing, offset: 0x30 });
        let status_set = status & 2 != 0;
        let phase = if !status_set || self.state.phase_count() != PhaseCount(0) {
            let token = hardware.enter_critical();
            self.release(handler, hardware);
            if self.state.irq_handle(handler) == Handle(0) && self.state.activity() == Activity::Active {
                let handle = hardware.configure(handler.source(status_set), handler).unwrap_or(Handle(0));
                self.state.set_irq_handle(handler, handle);
            }
            hardware.leave_critical(token);
            if status_set { Phase::Zero } else { Phase::One }
        } else {
            if self.state.activity() == Activity::Inactive { self.release(handler, hardware); }
            Phase::Zero
        };
        for slot in SlotIndex::ALL {
            if let Some(callback) = self.state.slot_callback(List::OneShot, slot) {
                if self.state.slot_phase(List::OneShot, slot).matches(phase) {
                    if phase == Phase::One { self.state.set_phase_count(PhaseCount(self.state.phase_count().0.wrapping_sub(1))); }
                    self.state.set_slot_callback(List::OneShot, slot, None);
                    self.state.set_slot_order(List::OneShot, slot, Order(0));
                    self.state.set_slot_handle(List::OneShot, slot, Handle(0));
                    self.state.set_slot_phase(List::OneShot, slot, SlotPhase::from_phase(Phase::Zero));
                    callbacks.invoke(callback, self, hardware);
                }
            }
        }
        let mut activity = Activity::Inactive;
        for slot in SlotIndex::ALL {
            if self.state.slot_handle(List::Persistent, slot) == Handle(0) { break; }
            activity = Activity::Active;
            if let Some(callback) = self.state.slot_callback(List::Persistent, slot) {
                if self.state.slot_phase(List::Persistent, slot).matches(phase) { callbacks.invoke(callback, self, hardware); }
            }
        }
        self.state.set_activity(activity);
    }
}
