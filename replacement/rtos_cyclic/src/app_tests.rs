use super::*;
use crate::tests::Mock;
use core::cell::{Cell, RefCell};

const SLOTS: GuestAddress = GuestAddress(0x409c_3500);
const TOKEN: GuestAddress = GuestAddress(0x0096_ca4c);
const PERIOD: u32 = 30;
const CALLBACK: u32 = 0x0069_cc18;
const SLOT64: u32 = 64;
const SLOT63: u32 = 63;
const SLOT62: u32 = 62;
const CREATE_FAILURE: i32 = -41;
const START_FAILURE: i32 = -5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Call {
    Current,
    Enter,
    Create(EncodedId, CyclicCreate),
    Start(EncodedId),
    Stop(EncodedId),
    Delete(EncodedId),
    Leave,
    Error(u32, i32, i32),
}

struct Adapter<'a> {
    mock: &'a Mock,
    interface: Cell<u32>,
    calls: RefCell<Vec<Call>>,
    create: Cell<Option<i32>>,
    start: Cell<Option<i32>>,
}

impl<'a> Adapter<'a> {
    fn new(mock: &'a Mock) -> Self {
        Self {
            mock,
            interface: Cell::new(0),
            calls: RefCell::new(Vec::new()),
            create: Cell::new(None),
            start: Cell::new(None),
        }
    }

    fn calls(&self) -> Vec<Call> {
        self.calls.borrow().clone()
    }
}

impl CallbackMachine for Adapter<'_> {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.mock.word(at.0)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.mock.set_word(at.0, value);
    }

    fn current_interface(&self) -> InterfaceId {
        self.calls.borrow_mut().push(Call::Current);
        InterfaceId(self.interface.get())
    }

    fn create_callback(
        &self,
        _interface: InterfaceId,
        id: EncodedId,
        request: CyclicCreate,
    ) -> CallbackStatus {
        self.calls.borrow_mut().push(Call::Create(id, request));
        match self.create.get() {
            Some(code) => CallbackStatus::from_code(code),
            None => CallbackStatus::from_code(create(self.mock, Domain::A, id, request).code()),
        }
    }

    fn start_callback(&self, _interface: InterfaceId, id: EncodedId) -> CallbackStatus {
        self.calls.borrow_mut().push(Call::Start(id));
        match self.start.get() {
            Some(code) => CallbackStatus::from_code(code),
            None => CallbackStatus::from_code(start(self.mock, Domain::A, id).code()),
        }
    }

    fn stop_callback(&self, _interface: InterfaceId, id: EncodedId) {
        self.calls.borrow_mut().push(Call::Stop(id));
        let _ = stop(self.mock, Domain::A, id);
    }

    fn delete_callback(&self, _interface: InterfaceId, id: EncodedId) {
        self.calls.borrow_mut().push(Call::Delete(id));
        let _ = delete(self.mock, Domain::A, id);
    }

    fn enter_critical(&self, _interface: InterfaceId) -> CriticalToken {
        self.calls.borrow_mut().push(Call::Enter);
        CriticalToken(0x5a5a)
    }

    fn leave_critical(&self, _interface: InterfaceId, _previous: CriticalToken) {
        self.calls.borrow_mut().push(Call::Leave);
    }

    fn record_error(&self, operation: u32, mode: i32, status: CallbackStatus) {
        self.calls
            .borrow_mut()
            .push(Call::Error(operation, mode, status.code()));
    }
}

fn slots() -> CallbackSlots {
    CallbackSlots::at(SLOTS)
}

fn mock_with_slot(slot: u32) -> Mock {
    let mock = Mock::new();
    let context = Domain::A.words().context(Domain::A);
    mock.set_word(context.cyclic_count().0, CallbackSlot::COUNT);
    mock.set_word(SLOTS.0, CallbackToken::FIRST as u32);
    mock.seed_control(slot, 0, 0);
    mock
}

fn register(adapter: &Adapter<'_>) -> CallbackStatus {
    register_callback(
        adapter,
        slots(),
        PeriodWord(PERIOD),
        0,
        DISPATCH_EVENT,
        CallbackHandler::new(CALLBACK),
        TokenOutput::at(TOKEN),
    )
}

fn slot_word(base: u32, slot: u32) -> u32 {
    SLOTS.0 + base + (slot - 1) * 4
}

fn busy(mock: &Mock, slot: u32) -> u32 {
    mock.word(slot_word(CallbackSlots::BUSY, slot))
}

fn set_busy(mock: &Mock, slot: u32, value: u32) {
    mock.set_word(slot_word(CallbackSlots::BUSY, slot), value);
}

fn registry_token(mock: &Mock, slot: u32) -> u32 {
    mock.word(slot_word(CallbackSlots::TOKENS, slot))
}

fn set_registry_token(mock: &Mock, slot: u32, value: u32) {
    mock.set_word(slot_word(CallbackSlots::TOKENS, slot), value);
}

fn request(id: u32) -> CyclicCreate {
    CyclicCreate {
        attributes: CyclicAttributes(0),
        argument: CyclicArgument(id),
        handler: DISPATCH_EVENT,
        period: PeriodWord(PERIOD),
        phase: CyclicPhase(PERIOD),
    }
}

#[test]
fn register_writes_the_request_registry_and_token() {
    let mock = mock_with_slot(SLOT64);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    let id = EncodedId::of(1, SLOT64 as u16);
    assert_eq!(mock.word(TOKEN.0), 30001);
    assert_eq!(busy(&mock, SLOT64), 1);
    assert_eq!(
        mock.word(slot_word(CallbackSlots::CALLBACKS, SLOT64)),
        CALLBACK
    );
    assert_eq!(registry_token(&mock, SLOT64), 30001);
    assert_eq!(
        mock.word(slot_word(CallbackSlots::ARGUMENTS, SLOT64)),
        PERIOD
    );
    assert_eq!(mock.word(SLOTS.0), 30002);
    let config = mock.config(SLOT64);
    assert_eq!(
        [
            mock.word(config.attributes().0),
            mock.word(config.argument().0),
            mock.word(config.handler().0),
            mock.word(config.period().0),
            mock.word(config.phase().0),
            mock.word(config.exists().0),
        ],
        [0, SLOT64, DISPATCH_EVENT.0, PERIOD, PERIOD, 1]
    );
    let control = mock.control(SLOT64);
    assert_eq!(mock.word(control.active().0), 1);
    assert_eq!(mock.word(control.expiry().0), PERIOD);
    assert_eq!(mock.count(), 1);
    assert_eq!(mock.entry_index(SLOT64), 1);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Enter,
            Call::Create(id, request(SLOT64)),
            Call::Leave,
            Call::Start(id),
        ]
    );
}

#[test]
fn register_prefers_slot_64_then_the_highest_free_slot() {
    let mock = mock_with_slot(SLOT63);
    set_busy(&mock, SLOT64, 1);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    assert_eq!(busy(&mock, SLOT63), 1);
    assert_eq!(registry_token(&mock, SLOT63), 30001);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Enter,
            Call::Create(
                EncodedId::of(1, SLOT63 as u16),
                request(SLOT63)
            ),
            Call::Leave,
            Call::Start(EncodedId::of(1, SLOT63 as u16)),
        ]
    );
}

#[test]
fn register_refuses_when_every_slot_is_busy() {
    let mock = Mock::new();
    for slot in 1..=SLOT64 {
        set_busy(&mock, slot, 1);
    }
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::NO_SLOT);
    assert_eq!(mock.word(TOKEN.0), 0xffff_ffff);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Enter,
            Call::Leave,
            Call::Error(ERROR_OPERATION, 0, CallbackStatus::NO_SLOT.code()),
        ]
    );
}

#[test]
fn register_refuses_an_unsupported_mode_before_the_critical_section() {
    let mock = Mock::new();
    let adapter = Adapter::new(&mock);
    let status = register_callback(
        &adapter,
        slots(),
        PeriodWord(PERIOD),
        1,
        DISPATCH_EVENT,
        CallbackHandler::new(CALLBACK),
        TokenOutput::at(TOKEN),
    );
    assert_eq!(status, CallbackStatus::INVALID);
    assert_eq!(mock.word(TOKEN.0), 0xffff_ffff);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Error(ERROR_OPERATION, 1, CallbackStatus::INVALID.code()),
        ]
    );
}

#[test]
fn register_refuses_a_null_callback() {
    let mock = Mock::new();
    let adapter = Adapter::new(&mock);
    let status = register_callback(
        &adapter,
        slots(),
        PeriodWord(PERIOD),
        0,
        DISPATCH_EVENT,
        CallbackHandler::new(0),
        TokenOutput::at(TOKEN),
    );
    assert_eq!(status, CallbackStatus::INVALID);
    assert_eq!(mock.word(TOKEN.0), 0xffff_ffff);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Error(ERROR_OPERATION, 0, CallbackStatus::INVALID.code()),
        ]
    );
}

#[test]
fn register_refuses_a_null_token_without_writing_address_zero() {
    let mock = Mock::new();
    let adapter = Adapter::new(&mock);
    let status = register_callback(
        &adapter,
        slots(),
        PeriodWord(PERIOD),
        0,
        DISPATCH_EVENT,
        CallbackHandler::new(CALLBACK),
        TokenOutput::at(GuestAddress(0)),
    );
    assert_eq!(status, CallbackStatus::INVALID);
    assert_eq!(mock.word(0), 0);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Error(ERROR_OPERATION, 0, CallbackStatus::INVALID.code()),
        ]
    );
}

#[test]
fn register_reports_a_create_failure_and_leaves_the_slot_free() {
    let mock = mock_with_slot(SLOT64);
    let adapter = Adapter::new(&mock);
    adapter.create.set(Some(CREATE_FAILURE));
    assert_eq!(register(&adapter), CallbackStatus::from_code(CREATE_FAILURE));
    assert_eq!(busy(&mock, SLOT64), 0);
    assert_eq!(mock.word(TOKEN.0), 0xffff_ffff);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Enter,
            Call::Create(EncodedId::of(1, SLOT64 as u16), request(SLOT64)),
            Call::Leave,
            Call::Error(ERROR_OPERATION, 0, CREATE_FAILURE),
        ]
    );
}

#[test]
fn register_reports_a_start_failure_and_keeps_the_reserved_slot() {
    let mock = mock_with_slot(SLOT64);
    let adapter = Adapter::new(&mock);
    adapter.start.set(Some(START_FAILURE));
    assert_eq!(register(&adapter), CallbackStatus::from_code(START_FAILURE));
    assert_eq!(busy(&mock, SLOT64), 1);
    assert_eq!(registry_token(&mock, SLOT64), 30001);
    assert_eq!(mock.word(TOKEN.0), 30001);
    assert_eq!(mock.word(mock.config(SLOT64).exists().0), 1);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Enter,
            Call::Create(EncodedId::of(1, SLOT64 as u16), request(SLOT64)),
            Call::Leave,
            Call::Start(EncodedId::of(1, SLOT64 as u16)),
            Call::Error(ERROR_OPERATION, 0, START_FAILURE),
        ]
    );
}

#[test]
fn register_allocates_past_used_token_candidates() {
    let mock = mock_with_slot(SLOT64);
    mock.set_word(SLOTS.0, 30001);
    set_registry_token(&mock, 1, 30001);
    set_registry_token(&mock, 2, 30002);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    assert_eq!(mock.word(TOKEN.0), 30003);
    assert_eq!(registry_token(&mock, SLOT64), 30003);
    assert_eq!(mock.word(SLOTS.0), 30004);
}

#[test]
fn register_wraps_the_token_sequence() {
    let mock = mock_with_slot(SLOT64);
    mock.set_word(SLOTS.0, CallbackToken::LAST as u32);
    set_registry_token(&mock, 1, CallbackToken::LAST as u32);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    assert_eq!(mock.word(TOKEN.0), CallbackToken::FIRST as u32);
    assert_eq!(registry_token(&mock, SLOT64), CallbackToken::FIRST as u32);
    assert_eq!(mock.word(SLOTS.0), (CallbackToken::FIRST + 1) as u32);
}

#[test]
fn unregister_stops_and_deletes_the_found_slot() {
    let mock = mock_with_slot(SLOT64);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    let token = CallbackToken::new(mock.word(TOKEN.0) as i32);
    adapter.calls.borrow_mut().clear();
    assert_eq!(
        unregister_callback(&adapter, slots(), token),
        CallbackStatus::OK
    );
    assert_eq!(busy(&mock, SLOT64), 0);
    assert_eq!(registry_token(&mock, SLOT64), 0);
    assert_eq!(mock.word(mock.config(SLOT64).exists().0), 0);
    assert_eq!(mock.word(mock.control(SLOT64).active().0), 0);
    assert_eq!(mock.count(), 0);
    assert_eq!(
        adapter.calls(),
        [
            Call::Current,
            Call::Stop(EncodedId::of(1, SLOT64 as u16)),
            Call::Delete(EncodedId::of(1, SLOT64 as u16)),
            Call::Enter,
            Call::Leave,
        ]
    );
}

#[test]
fn unregister_keeps_the_callback_and_argument_words() {
    let mock = mock_with_slot(SLOT64);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    let token = CallbackToken::new(mock.word(TOKEN.0) as i32);
    assert_eq!(
        unregister_callback(&adapter, slots(), token),
        CallbackStatus::OK
    );
    assert_eq!(
        mock.word(slot_word(CallbackSlots::CALLBACKS, SLOT64)),
        CALLBACK
    );
    assert_eq!(
        mock.word(slot_word(CallbackSlots::ARGUMENTS, SLOT64)),
        PERIOD
    );
}

#[test]
fn unregister_ignores_a_missing_token() {
    let mock = Mock::new();
    let adapter = Adapter::new(&mock);
    assert_eq!(
        unregister_callback(&adapter, slots(), CallbackToken::new(12345)),
        CallbackStatus::OK
    );
    assert_eq!(adapter.calls(), [Call::Current]);
}

#[test]
fn register_after_unregister_reuses_the_slot_and_advances_the_token() {
    let mock = mock_with_slot(SLOT64);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    let first = CallbackToken::new(mock.word(TOKEN.0) as i32);
    assert_eq!(
        unregister_callback(&adapter, slots(), first),
        CallbackStatus::OK
    );
    assert_eq!(register(&adapter), CallbackStatus::OK);
    assert_eq!(busy(&mock, SLOT64), 1);
    assert_eq!(mock.word(TOKEN.0), 30002);
    assert_eq!(registry_token(&mock, SLOT64), 30002);
    assert_eq!(mock.word(mock.config(SLOT64).period().0), PERIOD);
    assert_eq!(mock.word(mock.config(SLOT64).phase().0), PERIOD);
    assert_eq!(mock.word(mock.control(SLOT64).expiry().0), PERIOD);
    assert_eq!(mock.count(), 1);
}

#[test]
fn register_uses_slot_62_when_64_and_63_are_busy() {
    let mock = mock_with_slot(SLOT62);
    set_busy(&mock, SLOT64, 1);
    set_busy(&mock, SLOT63, 1);
    let adapter = Adapter::new(&mock);
    assert_eq!(register(&adapter), CallbackStatus::OK);
    assert_eq!(busy(&mock, SLOT62), 1);
    assert_eq!(registry_token(&mock, SLOT62), 30001);
    assert_eq!(
        adapter.calls()[2],
        Call::Create(EncodedId::of(1, SLOT62 as u16), request(SLOT62))
    );
}
