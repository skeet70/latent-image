use super::*;
use crate::layout::{CyclicConfig, CyclicControl, Domain, GuestAddress, KernelContext};
use core::cell::{Cell, RefCell};
use std::collections::BTreeMap;

use xm1_rtos_timer_host as timer;

const CONTEXT: u32 = 0x4005_45cc;
const SECOND_CONTEXT: u32 = 0x4005_4650;
const CONFIG_BASE: u32 = 0x4005_ef78;
const CONTROL_BASE: u32 = 0x4005_928c;
const SECOND_CONTROL_BASE: u32 = 0x4005_9300;
const HEAP: u32 = 0x4010_0000;
const HEAP_COUNT: u32 = 0x4010_1000;
const HEAP_BASE: u32 = 0x4010_1004;
const TICK: u32 = 0x4010_1008;
const LOCK_WORD: u32 = 0x4010_1014;
const LOCK_ACQUIRE: u32 = 0x4010_1020;
const LOCK_RELEASE: u32 = 0x4010_1024;
const EXPIRE_CALLBACK: u32 = 0x4beb0;
const HANDLER: u32 = 0x4010_1030;
const ARGUMENT: u32 = 0x1234_5678;

struct Mock {
    words: RefCell<BTreeMap<u32, u32>>,
    lock_held: Cell<u32>,
    i_masked: Cell<bool>,
    f_masked: Cell<bool>,
    handler_context: Cell<u32>,
    current: Cell<u32>,
    events: RefCell<Vec<(u32, u32)>>,
    seen: RefCell<Vec<(u32, u32, bool, bool)>>,
    action: RefCell<Option<fn(&Mock, u32, u32)>>,
}

impl Mock {
    fn new() -> Self {
        let mock = Self {
            words: RefCell::new(BTreeMap::new()),
            lock_held: Cell::new(0),
            i_masked: Cell::new(false),
            f_masked: Cell::new(false),
            handler_context: Cell::new(0),
            current: Cell::new(CONTEXT),
            events: RefCell::new(Vec::new()),
            seen: RefCell::new(Vec::new()),
            action: RefCell::new(None),
        };
        let words = Domain::A.words();
        assert_eq!(words.cyclic_configs.0, CONFIG_BASE);
        assert_eq!(words.cyclic_controls.0, CONTROL_BASE);
        assert_eq!(words.context(Domain::A).0 .0, CONTEXT);
        assert_eq!(words.context(Domain::B).0 .0, SECOND_CONTEXT);
        mock.set_word(words.handler_context.0, 0);
        mock.set_word(words.current_context.0, CONTEXT);
        mock.context_words(CONTEXT, CONTROL_BASE, 8);
        mock.context_words(SECOND_CONTEXT, SECOND_CONTROL_BASE, 8);
        mock.set_word(HEAP_COUNT, 0);
        mock.set_word(HEAP_BASE, 0);
        mock.set_word(TICK, 0);
        mock.set_word(LOCK_WORD, 0);
        mock
    }

    fn context_words(&self, context: u32, controls: u32, count: u32) {
        self.set_word(context + 0x34, HEAP);
        self.set_word(context + 0x38, HEAP_COUNT);
        self.set_word(context + 0x3c, HEAP_BASE);
        self.set_word(context + 0x40, TICK);
        self.set_word(context + 0x74, controls);
        self.set_word(context + 0x78, count);
        self.set_word(context + 0x80, EXPIRE_CALLBACK);
        self.set_word(context + 0x08, LOCK_WORD);
        self.set_word(context + 0x0c, LOCK_ACQUIRE);
        self.set_word(context + 0x10, LOCK_RELEASE);
    }

    fn set_word(&self, at: u32, value: u32) {
        self.words.borrow_mut().insert(at, value);
    }

    fn word(&self, at: u32) -> u32 {
        *self.words.borrow().get(&at).unwrap_or(&0)
    }

    fn config(&self, slot: u32) -> CyclicConfig {
        CyclicConfig(GuestAddress(CONFIG_BASE + (slot - 1) * 0x18))
    }

    fn control(&self, slot: u32) -> CyclicControl {
        CyclicControl(GuestAddress(CONTROL_BASE + (slot - 1) * 0x18))
    }

    fn second_control(&self, slot: u32) -> CyclicControl {
        CyclicControl(GuestAddress(SECOND_CONTROL_BASE + (slot - 1) * 0x18))
    }

    fn seed_config(&self, slot: u32, attributes: u32, period: u32, handler: u32) {
        let config = self.config(slot);
        self.set_word(config.attributes().0, attributes);
        self.set_word(config.argument().0, ARGUMENT);
        self.set_word(config.handler().0, handler);
        self.set_word(config.period().0, period);
        self.set_word(config.phase().0, 0);
        self.set_word(config.exists().0, 1);
    }

    fn seed_control(&self, slot: u32, active: u32, expiry: u32) {
        let control = self.control(slot);
        self.set_word(control.config().0, self.config(slot).0 .0);
        self.set_word(control.active().0, active);
        self.set_word(control.expiry().0, expiry);
        self.set_word(control.timer().0, 0);
    }

    fn seed_second_control(&self, slot: u32, active: u32, expiry: u32) {
        let control = self.second_control(slot);
        self.set_word(control.config().0, self.config(slot).0 .0);
        self.set_word(control.active().0, active);
        self.set_word(control.expiry().0, expiry);
        self.set_word(control.timer().0, 0);
    }

    fn heap(&self) -> timer::TimerHeap {
        self.heap_at(CONTEXT)
    }

    fn heap_at(&self, context: u32) -> timer::TimerHeap {
        timer::TimerHeap::from_context(
            self,
            timer::layout::KernelContext(timer::layout::GuestAddress(context)),
            timer::Capacity::new(8).unwrap(),
        )
    }

    fn count(&self) -> u32 {
        self.word(HEAP_COUNT)
    }

    fn entry_index(&self, slot: u32) -> u32 {
        self.word(self.control(slot).timer().0)
    }

    fn run_tick(&self) {
        self.handler_context.set(1);
        timer::tick(
            self,
            |m| m.heap(),
            |m, callback, argument| {
                assert_eq!(callback.0, EXPIRE_CALLBACK);
                let _ = expire(m, CyclicControl(GuestAddress(argument.0)));
            },
        )
        .unwrap();
    }

    fn handler_action(&self, action: fn(&Mock, u32, u32)) {
        *self.action.borrow_mut() = Some(action);
    }

    fn events(&self) -> Vec<(u32, u32)> {
        self.events.borrow().clone()
    }

    fn seen(&self) -> Vec<(u32, u32, bool, bool)> {
        self.seen.borrow().clone()
    }
}

impl Machine for Mock {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.word(at.0)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.set_word(at.0, value);
    }

    fn mask_interrupts(&self) -> SavedInterrupts {
        let saved = (self.i_masked.get() as u32) << 7 | (self.f_masked.get() as u32) << 6;
        self.i_masked.set(true);
        self.f_masked.set(true);
        SavedInterrupts(saved)
    }

    fn restore_interrupts(&self, saved: SavedInterrupts) {
        self.i_masked.set(saved.0 & 0x80 != 0);
        self.f_masked.set(saved.0 & 0x40 != 0);
    }

    fn handler_context(&self) -> u32 {
        self.handler_context.get()
    }

    fn irq_masked(&self) -> bool {
        self.i_masked.get()
    }

    fn irq_set(&self) {
        self.i_masked.set(true);
    }

    fn irq_clear(&self) {
        self.i_masked.set(false);
    }

    fn current_context(&self) -> KernelContext {
        KernelContext(GuestAddress(self.current.get()))
    }

    fn context(&self, domain: Domain) -> KernelContext {
        Domain::A.words().context(domain)
    }

    fn acquire_lock(&self, context: KernelContext) -> bool {
        self.lock_held.set(context.0 .0);
        true
    }

    fn release_lock(&self, _context: KernelContext) {
        self.lock_held.set(0);
    }

    fn insert_timer(&self, context: KernelContext, entry: GuestAddress, expiry: u32) -> bool {
        timer::insert(
            self,
            self.heap_at(context.0 .0),
            timer::TimerEntry::at(timer::layout::GuestAddress(entry.0)),
            timer::Expiry(expiry),
        )
        .is_ok()
    }

    fn remove_timer(&self, context: KernelContext, entry: GuestAddress) -> bool {
        timer::remove(
            self,
            self.heap_at(context.0 .0),
            timer::TimerEntry::at(timer::layout::GuestAddress(entry.0)),
        )
        .is_ok()
    }

    fn invoke(&self, handler: CyclicHandler, argument: CyclicArgument) {
        self.events.borrow_mut().push((handler.0, argument.0));
        self.seen.borrow_mut().push((
            handler.0,
            argument.0,
            self.lock_held.get() != 0,
            self.i_masked.get(),
        ));
        if let Some(action) = *self.action.borrow() {
            action(self, handler.0, argument.0);
        }
    }
}

impl timer::Machine for Mock {
    fn load_word(&self, at: timer::layout::GuestAddress) -> u32 {
        self.word(at.0)
    }

    fn store_word(&self, at: timer::layout::GuestAddress, value: u32) {
        self.set_word(at.0, value);
    }
}

impl timer::Scheduler for Mock {
    fn handler_context(&self) -> u32 {
        self.handler_context.get()
    }

    fn irq_masked(&self) -> bool {
        self.i_masked.get()
    }

    fn release_lock(&self) {
        self.lock_held.set(0);
    }

    fn acquire_lock(&self) {
        self.lock_held.set(self.current.get());
    }

    fn irq_clear(&self) {
        self.i_masked.set(false);
    }

    fn irq_set(&self) {
        self.i_masked.set(true);
    }
}

fn id(slot: u16) -> EncodedId {
    EncodedId::of(0, slot)
}

fn create_request(period: u32) -> CyclicCreate {
    CyclicCreate {
        attributes: CyclicAttributes(0x2),
        argument: CyclicArgument(ARGUMENT),
        handler: CyclicHandler(HANDLER),
        period: PeriodWord(period),
        phase: CyclicPhase(0),
    }
}

#[test]
fn create_writes_the_five_words_and_exists_last() {
    let mock = Mock::new();
    assert_eq!(create(&mock, Domain::A, id(3), create_request(30)), CyclicStatus::Ok);
    let config = mock.config(3);
    assert_eq!(
        [
            mock.word(config.attributes().0),
            mock.word(config.argument().0),
            mock.word(config.handler().0),
            mock.word(config.period().0),
            mock.word(config.phase().0),
            mock.word(config.exists().0),
        ],
        [0x2, ARGUMENT, HANDLER, 30, 0, 1]
    );
}

#[test]
fn create_refuses_an_existing_record_and_a_zero_period_without_a_write() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    assert_eq!(create(&mock, Domain::A, id(1), create_request(40)), CyclicStatus::AlreadyExists);
    assert_eq!(mock.word(mock.config(1).period().0), 30);
    let config = mock.config(2);
    assert_eq!(create(&mock, Domain::A, id(2), create_request(0)), CyclicStatus::Param);
    assert_eq!(mock.word(config.exists().0), 0);
    assert_eq!(mock.word(config.handler().0), 0);
}

#[test]
fn delete_zeroes_the_config_and_stops_an_active_timer() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 30);
    assert_eq!(
        start(&mock, Domain::A, id(1)),
        CyclicStatus::Ok
    );
    assert_eq!(mock.count(), 1);
    assert_eq!(delete(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    let config = mock.config(1);
    assert_eq!(
        [
            mock.word(config.attributes().0),
            mock.word(config.argument().0),
            mock.word(config.handler().0),
            mock.word(config.period().0),
            mock.word(config.phase().0),
            mock.word(config.exists().0),
        ],
        [0, 0, 0, 0, 0, 0]
    );
    assert_eq!(mock.word(mock.control(1).active().0), 0);
    assert_eq!(mock.entry_index(1), 0);
    assert_eq!(mock.count(), 0);
}

#[test]
fn delete_refuses_an_out_of_range_id() {
    let mock = Mock::new();
    assert_eq!(delete(&mock, Domain::A, id(0)), CyclicStatus::InvalidIndex);
    assert_eq!(delete(&mock, Domain::A, id(65)), CyclicStatus::InvalidIndex);
}

#[test]
fn start_arms_at_now_plus_period_with_the_context_callback() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 0, 0);
    mock.set_word(TICK, 100);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    let control = mock.control(1);
    assert_eq!(mock.word(control.active().0), 1);
    assert_eq!(mock.word(control.expiry().0), 130);
    assert_eq!(mock.entry_index(1), 1);
    assert_eq!(mock.word(control.timer().0 + 4), EXPIRE_CALLBACK);
    assert_eq!(mock.word(control.timer().0 + 8), control.0 .0);
    assert_eq!(mock.count(), 1);
    assert!(!mock.i_masked.get());
}

#[test]
fn start_restart_removes_the_old_entry_and_rearms() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 0, 0);
    mock.set_word(TICK, 100);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    mock.set_word(TICK, 200);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    assert_eq!(mock.word(mock.control(1).expiry().0), 230);
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.entry_index(1), 1);
}

#[test]
fn start_and_stop_refuse_handler_context_and_a_masked_irq() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 0, 0);
    mock.handler_context.set(1);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Context);
    assert_eq!(stop(&mock, Domain::A, id(1)), CyclicStatus::Context);
    mock.handler_context.set(0);
    mock.i_masked.set(true);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Context);
    assert_eq!(stop(&mock, Domain::A, id(1)), CyclicStatus::Context);
}

#[test]
fn start_and_stop_refuse_an_invalid_class_slot_and_uncreated_record() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 0, 0);
    assert_eq!(start(&mock, Domain::A, EncodedId::of(3, 1)), CyclicStatus::InvalidClass);
    assert_eq!(start(&mock, Domain::A, id(9)), CyclicStatus::InvalidIndex);
    assert_eq!(stop(&mock, Domain::A, id(0)), CyclicStatus::InvalidIndex);
    mock.set_word(mock.config(2).exists().0, 0);
    mock.seed_control(2, 0, 0);
    assert_eq!(start(&mock, Domain::A, id(2)), CyclicStatus::NotCreated);
    assert_eq!(stop(&mock, Domain::A, id(2)), CyclicStatus::NotCreated);
}

#[test]
fn start_refuses_a_zero_period_without_arming() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 0, HANDLER);
    mock.seed_control(1, 0, 0);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Param);
    assert_eq!(mock.count(), 0);
    assert_eq!(mock.word(mock.control(1).active().0), 0);
}

#[test]
fn stop_clears_active_and_detaches_and_is_idempotent() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 0, 0);
    mock.set_word(TICK, 100);
    assert_eq!(start(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    assert_eq!(stop(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    assert_eq!(mock.word(mock.control(1).active().0), 0);
    assert_eq!(mock.entry_index(1), 0);
    assert_eq!(mock.count(), 0);
    assert_eq!(stop(&mock, Domain::A, id(1)), CyclicStatus::Ok);
    assert_eq!(mock.count(), 0);
}

#[test]
fn start_selects_the_class_context_and_config() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_second_control(1, 0, 0);
    mock.set_word(TICK, 50);
    assert_eq!(start(&mock, Domain::A, EncodedId::of(2, 1)), CyclicStatus::Ok);
    assert_eq!(mock.word(mock.second_control(1).active().0), 1);
    assert_eq!(mock.word(mock.second_control(1).expiry().0), 80);
    assert_eq!(mock.count(), 1);
}

#[test]
fn expire_rearms_before_the_callback_and_returns_with_the_lock_held() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    assert_eq!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10), true);
    mock.set_word(TICK, 10);
    mock.run_tick();
    assert_eq!(mock.events(), vec![(HANDLER, ARGUMENT)]);
    assert_eq!(mock.seen(), vec![(HANDLER, ARGUMENT, false, false)]);
    assert_eq!(mock.word(mock.control(1).active().0), 1);
    assert_eq!(mock.word(mock.control(1).expiry().0), 40);
    assert_eq!(mock.entry_index(1), 1);
    assert_eq!(mock.word(HEAP), 40);
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.word(HEAP_BASE), 10);
    assert_eq!(mock.word(TICK), 20);
    assert_eq!(mock.lock_held.get(), 0);
    assert!(!mock.i_masked.get());
}

#[test]
fn expire_with_a_zero_period_stops_and_delivers_once() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 0, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    assert_eq!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10), true);
    mock.set_word(TICK, 10);
    mock.run_tick();
    assert_eq!(mock.events(), vec![(HANDLER, ARGUMENT)]);
    assert_eq!(mock.word(mock.control(1).active().0), 0);
    assert_eq!(mock.entry_index(1), 0);
    assert_eq!(mock.count(), 0);
    assert_eq!(mock.word(HEAP_BASE), 10);
    assert_eq!(mock.word(TICK), 20);
}

fn stop_and_restart(mock: &Mock, _handler: u32, _argument: u32) {
    mock.handler_context.set(0);
    assert_eq!(stop(mock, Domain::A, id(1)), CyclicStatus::Ok);
    assert_eq!(start(mock, Domain::A, id(1)), CyclicStatus::Ok);
    mock.handler_context.set(1);
}

#[test]
fn a_reentrant_stop_and_restart_keeps_the_new_registration() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    assert_eq!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10), true);
    mock.set_word(TICK, 10);
    mock.handler_action(stop_and_restart);
    mock.run_tick();
    assert_eq!(mock.events(), vec![(HANDLER, ARGUMENT)]);
    assert_eq!(mock.word(mock.control(1).active().0), 1);
    assert_eq!(mock.word(mock.control(1).expiry().0), 40);
    assert_eq!(mock.entry_index(1), 1);
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.word(HEAP), 40);
    assert_eq!(mock.word(HEAP_BASE), 10);
    assert_eq!(mock.word(TICK), 20);
}

fn delete_and_recreate(mock: &Mock, _handler: u32, _argument: u32) {
    assert_eq!(delete(mock, Domain::A, id(1)), CyclicStatus::Ok);
    assert_eq!(
        create(
            mock,
            Domain::A,
            id(1),
            CyclicRequest::new(0x2, ARGUMENT, 0x9999, 50, 0)
        ),
        CyclicStatus::Ok
    );
    mock.handler_context.set(0);
    assert_eq!(start(mock, Domain::A, id(1)), CyclicStatus::Ok);
    mock.handler_context.set(1);
}

struct CyclicRequest;

impl CyclicRequest {
    fn new(attributes: u32, argument: u32, handler: u32, period: u32, phase: u32) -> CyclicCreate {
        CyclicCreate {
            attributes: CyclicAttributes(attributes),
            argument: CyclicArgument(argument),
            handler: CyclicHandler(handler),
            period: PeriodWord(period),
            phase: CyclicPhase(phase),
        }
    }
}

#[test]
fn a_handler_delete_and_recreate_does_not_leave_the_old_expiry_armed() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    assert_eq!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10), true);
    mock.set_word(TICK, 10);
    mock.handler_action(delete_and_recreate);
    mock.run_tick();
    assert_eq!(mock.events(), vec![(HANDLER, ARGUMENT)]);
    let config = mock.config(1);
    assert_eq!(mock.word(config.period().0), 50);
    assert_eq!(mock.word(config.handler().0), 0x9999);
    assert_eq!(mock.word(mock.control(1).active().0), 1);
    assert_eq!(mock.word(mock.control(1).expiry().0), 60);
    assert_eq!(mock.entry_index(1), 1);
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.word(HEAP), 60);
}

#[test]
fn a_handler_can_delete_but_start_and_stop_refuse_handler_context() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    assert_eq!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10), true);
    mock.set_word(TICK, 10);
    mock.handler_action(|mock, _, _| {
        assert_eq!(start(mock, Domain::A, id(1)), CyclicStatus::Context);
        assert_eq!(stop(mock, Domain::A, id(1)), CyclicStatus::Context);
        assert_eq!(delete(mock, Domain::A, id(1)), CyclicStatus::Ok);
    });
    mock.run_tick();
    assert_eq!(mock.word(mock.config(1).exists().0), 0);
    assert_eq!(mock.word(mock.control(1).active().0), 0);
    assert_eq!(mock.entry_index(1), 0);
    assert_eq!(mock.count(), 0);
}

#[test]
fn expire_reacquires_the_current_context_after_the_callback() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    assert_eq!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10), true);
    mock.set_word(TICK, 10);
    mock.handler_action(|mock, _, _| {
        mock.current.set(SECOND_CONTEXT);
    });
    mock.run_tick();
    assert_eq!(mock.lock_held.get(), 0);
    assert_eq!(mock.events(), vec![(HANDLER, ARGUMENT)]);
}

#[test]
fn initialize_starts_the_phase_and_skips_a_zero_period() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.set_word(mock.config(1).phase().0, 7);
    mock.seed_config(2, 0x0, 30, HANDLER);
    mock.seed_config(3, 0x2, 0, HANDLER);
    mock.set_word(TICK, 100);
    initialize(
        &mock,
        KernelContext(GuestAddress(CONTEXT)),
        GuestAddress(CONFIG_BASE),
        GuestAddress(CONTROL_BASE),
        3,
    );
    assert_eq!(mock.word(mock.control(1).config().0), mock.config(1).0 .0);
    assert_eq!(mock.word(mock.control(1).active().0), 1);
    assert_eq!(mock.word(mock.control(1).expiry().0), 7);
    assert_eq!(mock.word(mock.control(2).active().0), 0);
    assert_eq!(mock.word(mock.control(3).active().0), 0);
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.word(HEAP), 7);
}

#[test]
fn configure_publishes_the_table_count_and_callback() {
    let mock = Mock::new();
    let configuration = GuestAddress(0x400c_0818);
    configure(&mock, configuration, GuestAddress(CONTROL_BASE), 8, EXPIRE_CALLBACK);
    assert_eq!(mock.word(configuration.0 + 0x60), CONTROL_BASE);
    assert_eq!(mock.word(configuration.0 + 0x64), 8);
    assert_eq!(mock.word(configuration.0 + 0x6c), EXPIRE_CALLBACK);
}

#[test]
fn detached_removal_does_not_underflow_the_heap() {
    let mock = Mock::new();
    mock.seed_config(1, 0x2, 30, HANDLER);
    mock.seed_control(1, 1, 10);
    mock.seed_config(2, 0x2, 40, HANDLER);
    mock.seed_control(2, 1, 20);
    mock.set_word(mock.control(1).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(1).timer().0 + 8, mock.control(1).0 .0);
    mock.set_word(mock.control(2).timer().0 + 4, EXPIRE_CALLBACK);
    mock.set_word(mock.control(2).timer().0 + 8, mock.control(2).0 .0);
    assert!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(1).timer(), 10));
    assert!(mock.insert_timer(KernelContext(GuestAddress(CONTEXT)), mock.control(2).timer(), 20));
    mock.set_word(TICK, 10);
    mock.handler_action(|mock, _, _| {
        mock.handler_context.set(0);
        assert_eq!(stop(mock, Domain::A, id(1)), CyclicStatus::Ok);
        assert_eq!(mock.count(), 1);
        mock.handler_context.set(1);
    });
    mock.run_tick();
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.entry_index(1), 0);
    assert_eq!(mock.entry_index(2), 1);
}
