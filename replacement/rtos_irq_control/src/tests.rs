use super::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Event {
    Load(u32),
    Store(u32, u32),
    MaskIrq,
    RestoreIrq(u32),
    MaskIrqFiq,
    RestoreIrqFiq(u32),
}

struct Model {
    words: RefCell<BTreeMap<u32, u32>>,
    events: RefCell<Vec<Event>>,
    cpsr: Cell<u32>,
    bank: Cell<Bank>,
}

impl Model {
    fn new(bank: Bank, cpsr: u32) -> Self {
        Self {
            words: RefCell::new(BTreeMap::new()),
            events: RefCell::new(Vec::new()),
            cpsr: Cell::new(cpsr),
            bank: Cell::new(bank),
        }
    }

    fn set(&self, address: u32, value: u32) {
        self.words.borrow_mut().insert(address, value);
    }

    fn get(&self, address: u32) -> u32 {
        self.words.borrow().get(&address).copied().unwrap_or(0)
    }

    fn events(&self) -> Vec<Event> {
        self.events.borrow().clone()
    }

    fn clear_events(&self) {
        self.events.borrow_mut().clear();
    }

    fn cpsr(&self) -> u32 {
        self.cpsr.get()
    }
}

impl Backend for Model {
    fn load_word(&self, at: GuestAddress) -> u32 {
        self.events.borrow_mut().push(Event::Load(at.0));
        self.get(at.0)
    }

    fn store_word(&self, at: GuestAddress, value: u32) {
        self.events.borrow_mut().push(Event::Store(at.0, value));
        self.set(at.0, value);
    }

    fn mask_irq(&self) -> SavedInterrupts {
        self.events.borrow_mut().push(Event::MaskIrq);
        let cpsr = self.cpsr.get();
        let saved = cpsr & 0x80;
        self.cpsr.set(cpsr | 0x80);
        SavedInterrupts(saved)
    }

    fn restore_irq(&self, saved: SavedInterrupts) {
        self.events.borrow_mut().push(Event::RestoreIrq(saved.0));
        let cpsr = self.cpsr.get();
        self.cpsr.set((cpsr & !0x80) | (saved.0 & 0x80));
    }

    fn mask_irq_fiq(&self) -> SavedInterrupts {
        self.events.borrow_mut().push(Event::MaskIrqFiq);
        let cpsr = self.cpsr.get();
        let saved = cpsr & 0xc0;
        self.cpsr.set(cpsr | 0xc0);
        SavedInterrupts(saved)
    }

    fn restore_irq_fiq(&self, saved: SavedInterrupts) {
        self.events.borrow_mut().push(Event::RestoreIrqFiq(saved.0));
        let cpsr = self.cpsr.get();
        self.cpsr.set((cpsr & !0xc0) | (saved.0 & 0xc0));
    }

    fn bank(&self) -> Bank {
        self.bank.get()
    }
}

const TABLE_A: u32 = 0x4005_38fc;
const TABLE_B: u32 = 0x400e_38bc;
const DEFAULT_HANDLER: u32 = 0x0004_2840;
const PACKET: u32 = 0x0096_c000;
const LINE: u32 = 5;
const BIT: u32 = 1 << LINE;

fn table(base: u32) -> Table {
    Table::new(GuestAddress(base), Handler::from_static(DEFAULT_HANDLER))
}

fn entry(base: u32) -> u32 {
    base + LINE * ENTRY_SIZE
}

fn set_entry(m: &Model, base: u32, handler: u32, word4: u32, priority: u32, word_c: u32, mode: u32) {
    let at = entry(base);
    m.set(at, handler);
    m.set(at + 0x04, word4);
    m.set(at + 0x08, priority);
    m.set(at + 0x0c, word_c);
    m.set(at + 0x10, mode);
}

fn set_packet(m: &Model, handler: u32, priority: u32, flag_c: u32, mode: u32) {
    m.set(PACKET, 0xdead_beef);
    m.set(PACKET + 0x04, handler);
    m.set(PACKET + 0x08, priority);
    m.set(PACKET + 0x0c, flag_c);
    m.set(PACKET + 0x10, mode);
}

fn bank_word(m: &Model, bank: Bank, offset: u32) -> u32 {
    m.get(bank.base() + offset)
}

fn set_bank_word(m: &Model, bank: Bank, offset: u32, value: u32) {
    m.set(bank.base() + offset, value);
}

fn seed_bank(m: &Model, bank: Bank) {
    set_bank_word(m, bank, GROUP_ENABLE, 0x1111_1111);
    set_bank_word(m, bank, GROUP_CLEAR, 0xaaaa_aaaa);
    set_bank_word(m, bank, GROUP_CONFIG_60, 0xf0f0_f0c0);
    set_bank_word(m, bank, GROUP_CONFIG_100, 0x0f0f_0f0f);
    set_bank_word(m, bank, GROUP_SET_PENDING, 0xbbbb_bbbb);
    set_bank_word(m, bank, GROUP_CLEAR_PENDING, 0xcccc_cccc);
    set_bank_word(m, bank, HALF_140, 0);
    set_bank_word(m, bank, HALF_180, 0xffff_ffff);
    set_bank_word(m, bank, VECTOR + 4 * LINE, 0xdead_beef);
    set_bank_word(m, bank, PRIORITY + 4 * LINE, 0xcafe_babe);
}

fn seed_other_bank(m: &Model, bank: Bank) {
    for offset in [GROUP_ENABLE, GROUP_CLEAR, GROUP_CONFIG_60, GROUP_CONFIG_100, HALF_140, HALF_180, VECTOR, PRIORITY] {
        set_bank_word(m, bank, offset, 0x5a5a_5a5a);
    }
}

#[test]
fn interrupt_line_bounds_and_named_counterexamples() {
    assert_eq!(InterruptLine::new(0).unwrap().get(), 0);
    assert_eq!(InterruptLine::new(127).unwrap().get(), 127);
    for value in [128, 129, 255, 256, 0x7fff_ffff, 0xffff_ffff] {
        assert!(InterruptLine::new(value).is_none(), "value {value}");
    }
    for raw in [-1, -2, -127, -128, i32::MIN] {
        assert!(InterruptLine::from_raw(raw).is_none(), "raw {raw}");
    }
    assert_eq!(InterruptLine::from_define_argument(0x1_0000).unwrap().get(), 0);
    assert!(InterruptLine::from_define_argument(0xffff).is_none());
    assert!(InterruptLine::from_define_argument((-1i32) as u32).is_none());
}

#[test]
fn registration_rejects_each_invalid_field() {
    assert!(Registration::new(0, 0, 0, 0).is_none());
    assert!(Registration::new(1, 16, 0, 0).is_none());
    assert!(Registration::new(1, 0, 2, 0).is_none());
    assert!(Registration::new(1, 0, 0, 4).is_none());
    assert_eq!(
        Registration::new(0x1234_5678, 15, 1, 3),
        Some(Registration {
            handler: Handler::from_static(0x1234_5678),
            priority: Priority::from_static(15),
            flag_c: Flag::from_static(1),
            mode: Mode::from_static(3),
        })
    );
    assert!(Handler::new(0).is_none());
    assert_eq!(Handler::new(0x42840).unwrap().get(), 0x42840);
}

#[test]
fn table_entries_are_24_bytes_apart() {
    let t = table(TABLE_A);
    assert_eq!(t.entry(InterruptLine::from_static(0)).0, TABLE_A);
    assert_eq!(t.entry(InterruptLine::from_static(1)).0, TABLE_A + 0x18);
    assert_eq!(t.entry(InterruptLine::from_static(127)).0, TABLE_A + 127 * 0x18);
}

#[test]
fn out_of_range_enable_and_disable_touch_nothing() {
    let m = Model::new(Bank::Primary, 0x13);
    seed_bank(&m, Bank::Primary);
    set_entry(&m, TABLE_A, 0x1234_5678, 1, 3, 1, 2);
    let before = m.events();
    assert_eq!(enable_raw(&m, table(TABLE_A), -1), Err(IrqError::OutOfRange));
    assert_eq!(enable_raw(&m, table(TABLE_A), -128), Err(IrqError::OutOfRange));
    assert_eq!(enable_raw(&m, table(TABLE_A), 128), Err(IrqError::OutOfRange));
    assert_eq!(disable_raw(&m, table(TABLE_A), -1), Err(IrqError::OutOfRange));
    assert_eq!(disable_raw(&m, table(TABLE_A), 0x7fff_ffff), Err(IrqError::OutOfRange));
    assert_eq!(m.events(), before);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn enable_publishes_the_entry_on_the_primary_bank() {
    let m = Model::new(Bank::Primary, 0x13);
    seed_bank(&m, Bank::Primary);
    seed_other_bank(&m, Bank::Secondary);
    set_entry(&m, TABLE_A, 0x1234_5678, 1, 3, 1, 2);
    let bank = Bank::Primary.base();
    let at = entry(TABLE_A);
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(
        m.events(),
        vec![
            Event::MaskIrq,
            Event::Load(at),
            Event::Load(bank + GROUP_ENABLE),
            Event::Store(bank + GROUP_CLEAR, 0xffff_ffff),
            Event::Load(bank + GROUP_CONFIG_60),
            Event::Load(at + 0x04),
            Event::Store(bank + GROUP_CONFIG_60, 0xf0f0_f0e0),
            Event::Load(bank + GROUP_CONFIG_100),
            Event::Load(at + 0x0c),
            Event::Store(bank + GROUP_CONFIG_100, 0x0f0f_0f2f),
            Event::Load(at + 0x0c),
            Event::Load(bank + HALF_180),
            Event::Store(bank + HALF_180, 0xffff_f3ff),
            Event::Load(bank + HALF_140),
            Event::Load(at + 0x10),
            Event::Store(bank + HALF_140, 2 << (2 * LINE)),
            Event::Load(at),
            Event::Store(bank + VECTOR + 4 * LINE, 0x1234_5678),
            Event::Load(at + 0x08),
            Event::Store(bank + PRIORITY + 4 * LINE, 3),
            Event::Store(bank + GROUP_SET_PENDING, BIT),
            Event::Store(bank + GROUP_ENABLE, 0x1111_1111 | BIT),
            Event::RestoreIrq(0),
        ]
    );
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_ENABLE), 0x1111_1111 | BIT);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CLEAR), 0xffff_ffff);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CONFIG_60), 0xf0f0_f0e0);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CONFIG_100), 0x0f0f_0f2f);
    assert_eq!(bank_word(&m, Bank::Primary, HALF_180), 0xffff_f3ff);
    assert_eq!(bank_word(&m, Bank::Primary, HALF_140), 2 << (2 * LINE));
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), 0x1234_5678);
    assert_eq!(bank_word(&m, Bank::Primary, PRIORITY + 4 * LINE), 3);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_SET_PENDING), BIT);
    assert_eq!(bank_word(&m, Bank::Secondary, GROUP_ENABLE), 0x5a5a_5a5a);
    assert_eq!(bank_word(&m, Bank::Secondary, VECTOR), 0x5a5a_5a5a);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn enable_uses_the_other_half_word_field_when_entry_flag_c_is_zero() {
    let m = Model::new(Bank::Primary, 0x13);
    seed_bank(&m, Bank::Primary);
    set_entry(&m, TABLE_A, 0x1234_5678, 0, 3, 0, 2);
    let bank = Bank::Primary.base();
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(bank_word(&m, Bank::Primary, HALF_180), 0xffff_fbff);
    assert_eq!(bank_word(&m, Bank::Primary, HALF_140), 0);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CONFIG_60), 0xf0f0_f0c0);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CONFIG_100), 0x0f0f_0f0f);
    assert_eq!(m.events()[11], Event::Load(bank + HALF_180));
    assert_eq!(m.events()[12], Event::Load(entry(TABLE_A) + 0x10));
    assert_eq!(m.events()[13], Event::Store(bank + HALF_180, 0xffff_fbff));
    assert_eq!(m.events()[14], Event::Load(bank + HALF_140));
    assert_eq!(m.events()[15], Event::Store(bank + HALF_140, 0));
}

#[test]
fn enable_refuses_the_empty_handler_without_a_bank_write() {
    let m = Model::new(Bank::Primary, 0x13);
    seed_bank(&m, Bank::Primary);
    set_entry(&m, TABLE_A, DEFAULT_HANDLER, 0, 15, 0, 1);
    let before = m.words.borrow().clone();
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Err(IrqError::NoHandler));
    assert_eq!(m.events(), vec![Event::MaskIrq, Event::Load(entry(TABLE_A)), Event::RestoreIrq(0)]);
    assert_eq!(*m.words.borrow(), before);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn disable_clears_the_line_and_writes_the_empty_handler() {
    let m = Model::new(Bank::Primary, 0x13);
    seed_bank(&m, Bank::Primary);
    seed_other_bank(&m, Bank::Secondary);
    set_entry(&m, TABLE_A, 0x1234_5678, 1, 3, 1, 2);
    let bank = Bank::Primary.base();
    assert_eq!(disable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(
        m.events(),
        vec![
            Event::MaskIrq,
            Event::Load(bank + GROUP_ENABLE),
            Event::Store(bank + GROUP_CLEAR, 0xffff_ffff),
            Event::Load(bank + GROUP_CONFIG_60),
            Event::Store(bank + GROUP_CONFIG_60, 0xf0f0_f0c0),
            Event::Store(bank + GROUP_CLEAR_PENDING, BIT),
            Event::Load(bank + GROUP_CONFIG_100),
            Event::Store(bank + GROUP_CONFIG_100, 0x0f0f_0f0f),
            Event::Store(bank + GROUP_SET_PENDING, BIT),
            Event::Load(bank + HALF_180),
            Event::Store(bank + HALF_180, 0xffff_f3ff),
            Event::Load(bank + HALF_140),
            Event::Store(bank + HALF_140, 0),
            Event::Store(bank + VECTOR + 4 * LINE, DEFAULT_HANDLER),
            Event::Store(bank + GROUP_ENABLE, 0x1111_1111 & !BIT),
            Event::RestoreIrq(0),
        ]
    );
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_ENABLE), 0x1111_1111 & !BIT);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CLEAR), 0xffff_ffff);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CONFIG_60), 0xf0f0_f0c0);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CLEAR_PENDING), BIT);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_CONFIG_100), 0x0f0f_0f0f);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_SET_PENDING), BIT);
    assert_eq!(bank_word(&m, Bank::Primary, HALF_180), 0xffff_f3ff);
    assert_eq!(bank_word(&m, Bank::Primary, HALF_140), 0);
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), DEFAULT_HANDLER);
    assert_eq!(bank_word(&m, Bank::Primary, PRIORITY + 4 * LINE), 0xcafe_babe);
    assert_eq!(bank_word(&m, Bank::Secondary, GROUP_ENABLE), 0x5a5a_5a5a);
    assert_eq!(m.get(entry(TABLE_A)), 0x1234_5678);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn define_writes_the_registration_and_no_controller_word() {
    let m = Model::new(Bank::Primary, 0x13);
    seed_bank(&m, Bank::Primary);
    set_packet(&m, 0x1234_5678, 3, 1, 2);
    let at = entry(TABLE_A);
    assert_eq!(
        define(&m, table(TABLE_A), LINE, Some(GuestAddress(PACKET))),
        DefineOutcome::Ok
    );
    assert_eq!(
        m.events(),
        vec![
            Event::MaskIrqFiq,
            Event::Load(PACKET + 0x04),
            Event::Load(PACKET + 0x08),
            Event::Load(PACKET + 0x0c),
            Event::Load(PACKET + 0x10),
            Event::MaskIrq,
            Event::Store(at, 0x1234_5678),
            Event::Store(at + 0x04, 0),
            Event::Store(at + 0x08, 3),
            Event::Store(at + 0x0c, 1),
            Event::Store(at + 0x10, 2),
            Event::RestoreIrq(0x80),
            Event::RestoreIrqFiq(0),
        ]
    );
    assert_eq!(m.get(at), 0x1234_5678);
    assert_eq!(m.get(at + 0x04), 0);
    assert_eq!(m.get(at + 0x08), 3);
    assert_eq!(m.get(at + 0x0c), 1);
    assert_eq!(m.get(at + 0x10), 2);
    assert_eq!(bank_word(&m, Bank::Primary, GROUP_ENABLE), 0x1111_1111);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn define_null_installs_the_default_registration() {
    let m = Model::new(Bank::Primary, 0x13);
    let at = entry(TABLE_A);
    assert_eq!(define(&m, table(TABLE_A), LINE, None), DefineOutcome::Ok);
    assert_eq!(
        m.events(),
        vec![
            Event::MaskIrqFiq,
            Event::MaskIrq,
            Event::Store(at, DEFAULT_HANDLER),
            Event::Store(at + 0x04, 0),
            Event::Store(at + 0x08, 15),
            Event::Store(at + 0x0c, 0),
            Event::Store(at + 0x10, 1),
            Event::RestoreIrq(0x80),
            Event::RestoreIrqFiq(0),
        ]
    );
    assert_eq!(m.get(at), DEFAULT_HANDLER);
    assert_eq!(m.get(at + 0x08), 15);
    assert_eq!(m.get(at + 0x10), 1);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn define_refuses_each_invalid_packet_without_a_table_write() {
    let cases = [
        (0u32, 3u32, 1u32, 2u32, LINE),
        (0x1234_5678, 16, 1, 2, LINE),
        (0x1234_5678, 3, 2, 2, LINE),
        (0x1234_5678, 3, 1, 4, LINE),
        (0x1234_5678, 3, 1, 2, 128),
    ];
    for (handler, priority, flag_c, mode, line) in cases {
        let m = Model::new(Bank::Primary, 0x13);
        set_packet(&m, handler, priority, flag_c, mode);
        let before = m.words.borrow().clone();
        assert_eq!(
            define(&m, table(TABLE_A), line, Some(GuestAddress(PACKET))),
            DefineOutcome::Refused,
            "handler {handler:#x} priority {priority} flag {flag_c} mode {mode} line {line}"
        );
        assert_eq!(m.events()[0], Event::MaskIrqFiq);
        assert_eq!(m.events().last(), Some(&Event::RestoreIrqFiq(0)));
        assert!(!m.events().iter().any(|event| matches!(event, Event::MaskIrq)));
        assert!(!m.events().iter().any(|event| matches!(event, Event::Store(_, _))));
        assert_eq!(*m.words.borrow(), before);
        assert_eq!(m.cpsr(), 0x13);
    }
}

#[test]
fn define_null_with_an_out_of_range_line_returns_ok_without_a_write() {
    for line in [128u32, 129, 0xffff] {
        let m = Model::new(Bank::Primary, 0x13);
        let before = m.words.borrow().clone();
        assert_eq!(define(&m, table(TABLE_A), line, None), DefineOutcome::Ok);
        assert_eq!(m.events(), vec![Event::MaskIrqFiq, Event::RestoreIrqFiq(0)]);
        assert_eq!(*m.words.borrow(), before);
    }
}

#[test]
fn define_truncates_its_first_argument_to_16_bits() {
    let m = Model::new(Bank::Primary, 0x13);
    set_packet(&m, 0x1234_5678, 3, 1, 2);
    assert_eq!(
        define(&m, table(TABLE_A), 0x1_0000, Some(GuestAddress(PACKET))),
        DefineOutcome::Ok
    );
    assert_eq!(m.get(TABLE_A), 0x1234_5678);

    let m = Model::new(Bank::Primary, 0x13);
    set_packet(&m, 0x1234_5678, 3, 1, 2);
    let before = m.words.borrow().clone();
    assert_eq!(
        define(&m, table(TABLE_A), 0xffff_ffff, Some(GuestAddress(PACKET))),
        DefineOutcome::Refused
    );
    assert_eq!(*m.words.borrow(), before);
}

#[test]
fn define_then_enable_publishes_the_replacement_handler() {
    let m = Model::new(Bank::Primary, 0x13);
    set_packet(&m, 0x1111_1111, 3, 1, 2);
    assert_eq!(define(&m, table(TABLE_A), LINE, Some(GuestAddress(PACKET))), DefineOutcome::Ok);
    m.clear_events();
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), 0x1111_1111);

    set_packet(&m, 0x2222_2222, 4, 0, 3);
    assert_eq!(define(&m, table(TABLE_A), LINE, Some(GuestAddress(PACKET))), DefineOutcome::Ok);
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), 0x1111_1111);
    m.clear_events();
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), 0x2222_2222);
    assert_eq!(bank_word(&m, Bank::Primary, PRIORITY + 4 * LINE), 4);
}

#[test]
fn disable_leaves_the_table_and_enable_republishes_it() {
    let m = Model::new(Bank::Primary, 0x13);
    set_entry(&m, TABLE_A, 0x1234_5678, 1, 3, 1, 2);
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), 0x1234_5678);
    assert_eq!(disable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), DEFAULT_HANDLER);
    assert_eq!(m.get(entry(TABLE_A)), 0x1234_5678);
    assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR + 4 * LINE), 0x1234_5678);
}

#[test]
fn bank_follows_the_executing_core_not_the_table_domain() {
    for base in [TABLE_A, TABLE_B] {
        for selected in [Bank::Primary, Bank::Secondary] {
            let other = match selected {
                Bank::Primary => Bank::Secondary,
                Bank::Secondary => Bank::Primary,
            };
            let m = Model::new(selected, 0x13);
            seed_bank(&m, selected);
            seed_other_bank(&m, other);
            set_entry(&m, base, 0x1234_5678, 1, 3, 1, 2);
            assert_eq!(enable(&m, table(base), InterruptLine::from_static(LINE)), Ok(()));
            assert_eq!(bank_word(&m, selected, VECTOR + 4 * LINE), 0x1234_5678);
            assert_eq!(bank_word(&m, other, VECTOR), 0x5a5a_5a5a);
            assert_eq!(m.get(entry(base)), 0x1234_5678);
        }
    }
}

#[test]
fn services_preserve_the_caller_control_bits() {
    for cpsr in [0x13, 0x93, 0xd3, 0x1f, 0x12] {
        let m = Model::new(Bank::Primary, cpsr);
        set_entry(&m, TABLE_A, 0x1234_5678, 1, 3, 1, 2);
        assert_eq!(enable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
        assert_eq!(m.cpsr(), cpsr, "enable {cpsr:#x}");
        assert_eq!(disable(&m, table(TABLE_A), InterruptLine::from_static(LINE)), Ok(()));
        assert_eq!(m.cpsr(), cpsr, "disable {cpsr:#x}");
    }
    for cpsr in [0x13, 0x93, 0xd3, 0x1f] {
        let m = Model::new(Bank::Primary, cpsr);
        set_packet(&m, 0x1234_5678, 3, 1, 2);
        assert_eq!(
            define(&m, table(TABLE_A), LINE, Some(GuestAddress(PACKET))),
            DefineOutcome::Ok
        );
        assert_eq!(m.cpsr(), cpsr, "define {cpsr:#x}");
    }
}

#[test]
fn define_preserves_the_packet_words_it_does_not_own() {
    let m = Model::new(Bank::Primary, 0x13);
    set_packet(&m, 0x1234_5678, 3, 1, 2);
    assert_eq!(
        define(&m, table(TABLE_A), LINE, Some(GuestAddress(PACKET))),
        DefineOutcome::Ok
    );
    assert_eq!(m.get(PACKET), 0xdead_beef);
    assert!(!m.events().contains(&Event::Load(PACKET)));
}

#[test]
fn register_shift_uses_the_low_byte_and_saturates_at_32() {
    assert_eq!(shift_left_register(1, 0), 1);
    assert_eq!(shift_left_register(1, 31), 0x8000_0000);
    assert_eq!(shift_left_register(1, 32), 0);
    assert_eq!(shift_left_register(3, 0xff), 0);
    assert_eq!(shift_left_register(1, 0x105), 0x20);
    assert_eq!(shift_left_register(1, (-1i32) as u32), 0);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum CpsrEvent {
    Read,
    Mask(u32),
    Unmask(u32),
}

struct CpsrModel {
    cpsr: Cell<u32>,
    events: RefCell<Vec<CpsrEvent>>,
}

impl CpsrModel {
    fn new(cpsr: u32) -> Self {
        Self {
            cpsr: Cell::new(cpsr),
            events: RefCell::new(Vec::new()),
        }
    }
}

impl mask::Cpsr for CpsrModel {
    fn read_cpsr(&self) -> u32 {
        self.events.borrow_mut().push(CpsrEvent::Read);
        self.cpsr.get()
    }

    fn mask(&self, bits: mask::MaskBits) {
        self.events.borrow_mut().push(CpsrEvent::Mask(bits.bits()));
        self.cpsr.set(self.cpsr.get() | bits.bits());
    }

    fn unmask(&self, bits: mask::MaskBits) {
        self.events.borrow_mut().push(CpsrEvent::Unmask(bits.bits()));
        self.cpsr.set(self.cpsr.get() & !bits.bits());
    }
}

/// Stock `msr CPSR_c, (cpsr & ~mask) | (saved & mask)`.
fn stock_restore(cpsr: u32, bits: u32, saved: u32) -> u32 {
    (cpsr & !bits) | (saved & bits)
}

#[test]
fn mask_save_returns_the_prior_bits_and_masks_only_its_bits() {
    for (bits, kind) in [(0xc0, mask::MaskBits::IrqFiq), (0x80, mask::MaskBits::Irq), (0x40, mask::MaskBits::Fiq)] {
        for cpsr in [0x13, 0x53, 0x93, 0xd3, 0x1f, 0x3f] {
            let c = CpsrModel::new(cpsr);
            assert_eq!(mask::save(&c, kind), SavedInterrupts(cpsr & bits), "{bits:#x} {cpsr:#x}");
            assert_eq!(c.cpsr.get(), cpsr | bits, "{bits:#x} {cpsr:#x}");
        }
    }
}

#[test]
fn mask_restore_matches_stock_control_bits_and_returns_the_mask() {
    for (bits, kind) in [(0xc0, mask::MaskBits::IrqFiq), (0x80, mask::MaskBits::Irq), (0x40, mask::MaskBits::Fiq)] {
        for cpsr in [0x13, 0x53, 0x93, 0xd3, 0x3f] {
            for saved in [0x00, 0x40, 0x80, 0xc0, 0xffff_ffff, 0x13] {
                let c = CpsrModel::new(cpsr);
                assert_eq!(mask::restore(&c, kind, SavedInterrupts(saved)), bits);
                assert_eq!(c.cpsr.get(), stock_restore(cpsr, bits, saved), "{bits:#x} {cpsr:#x} {saved:#x}");
            }
        }
    }
}

#[test]
fn mask_restore_masks_before_it_unmasks() {
    let c = CpsrModel::new(0xd3);
    mask::restore(&c, mask::MaskBits::IrqFiq, SavedInterrupts(0x40));
    assert_eq!(*c.events.borrow(), vec![CpsrEvent::Mask(0x40), CpsrEvent::Unmask(0x80)]);
    let c = CpsrModel::new(0x13);
    mask::restore(&c, mask::MaskBits::IrqFiq, SavedInterrupts(0));
    assert_eq!(*c.events.borrow(), vec![CpsrEvent::Unmask(0xc0)]);
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Cp15Event {
    ReadSctlr,
    WriteSctlr(u32),
    ReadClidr,
    WriteCsselr(u32),
    ReadCcsidr,
    Maintain(cache::Maintenance, u32),
    Dsb,
    Isb,
}

struct Cp15Model {
    sctlr: Cell<u32>,
    clidr: u32,
    csselr: Cell<u32>,
    ccsidr: BTreeMap<u32, u32>,
    events: RefCell<Vec<Cp15Event>>,
}

impl Cp15Model {
    fn new(sctlr: u32, clidr: u32, ccsidr: &[(u32, u32)]) -> Self {
        Self {
            sctlr: Cell::new(sctlr),
            clidr,
            csselr: Cell::new(0),
            ccsidr: ccsidr.iter().copied().collect(),
            events: RefCell::new(Vec::new()),
        }
    }

    fn events(&self) -> Vec<Cp15Event> {
        self.events.borrow().clone()
    }

    fn push(&self, event: Cp15Event) {
        self.events.borrow_mut().push(event);
    }
}

impl cache::Cp15 for Cp15Model {
    fn read_sctlr(&self) -> u32 {
        self.push(Cp15Event::ReadSctlr);
        self.sctlr.get()
    }

    fn write_sctlr(&self, value: u32) {
        self.push(Cp15Event::WriteSctlr(value));
        self.sctlr.set(value);
    }

    fn read_clidr(&self) -> u32 {
        self.push(Cp15Event::ReadClidr);
        self.clidr
    }

    fn write_csselr(&self, value: u32) {
        self.push(Cp15Event::WriteCsselr(value));
        self.csselr.set(value);
    }

    fn read_ccsidr(&self) -> u32 {
        self.push(Cp15Event::ReadCcsidr);
        *self.ccsidr.get(&self.csselr.get()).expect("undeclared CSSELR")
    }

    fn maintain(&self, op: cache::Maintenance, operand: u32) {
        self.push(Cp15Event::Maintain(op, operand));
    }

    fn dsb(&self) {
        self.push(Cp15Event::Dsb);
    }

    fn isb(&self) {
        self.push(Cp15Event::Isb);
    }
}

use cache::Maintenance as Op;

#[test]
fn cache_enable_invalidates_the_other_cache_between_the_barrier_and_the_sctlr_write() {
    let c = Cp15Model::new(0x0000_0078, 0, &[]);
    assert_eq!(cache::enable(&c, cache::Cache::Instruction), 0x1000);
    assert_eq!(
        c.events(),
        vec![
            Cp15Event::ReadSctlr,
            Cp15Event::Dsb,
            Cp15Event::Maintain(Op::InvalidateDataAll, 0),
            Cp15Event::WriteSctlr(0x0000_1078),
            Cp15Event::Isb,
        ]
    );
    let c = Cp15Model::new(0x0000_1078, 0, &[]);
    assert_eq!(cache::enable(&c, cache::Cache::Data), 4);
    assert_eq!(
        c.events(),
        vec![
            Cp15Event::ReadSctlr,
            Cp15Event::Dsb,
            Cp15Event::Maintain(Op::InvalidateInstructionAll, 0),
            Cp15Event::WriteSctlr(0x0000_107c),
            Cp15Event::Isb,
        ]
    );
}

#[test]
fn cache_disable_clears_the_bit_between_dsb_and_isb() {
    let c = Cp15Model::new(0x0000_107d, 0, &[]);
    assert_eq!(cache::disable(&c, cache::Cache::Data), 4);
    assert_eq!(
        c.events(),
        vec![Cp15Event::ReadSctlr, Cp15Event::Dsb, Cp15Event::WriteSctlr(0x0000_1079), Cp15Event::Isb]
    );
    let c = Cp15Model::new(0x0000_107d, 0, &[]);
    assert_eq!(cache::disable(&c, cache::Cache::Instruction), 0x1000);
    assert_eq!(
        c.events(),
        vec![Cp15Event::ReadSctlr, Cp15Event::Dsb, Cp15Event::WriteSctlr(0x0000_007d), Cp15Event::Isb]
    );
}

#[test]
fn cache_range_walks_aligned_lines_without_barriers() {
    for (kind, op) in [
        (cache::RangeOp::InvalidateInstruction, Op::InvalidateInstructionLine),
        (cache::RangeOp::CleanInvalidateData, Op::CleanInvalidateDataLine),
        (cache::RangeOp::CleanData, Op::CleanDataLine),
        (cache::RangeOp::InvalidateData, Op::InvalidateDataLine),
    ] {
        let c = Cp15Model::new(0, 0, &[]);
        assert_eq!(cache::range(&c, kind, 0x0096_c010, 0x40), 0x0096_c060);
        assert_eq!(
            c.events(),
            vec![
                Cp15Event::Maintain(op, 0x0096_c000),
                Cp15Event::Maintain(op, 0x0096_c020),
                Cp15Event::Maintain(op, 0x0096_c040),
            ]
        );
    }
}

#[test]
fn cache_range_compares_signed_and_runs_at_least_once() {
    let c = Cp15Model::new(0, 0, &[]);
    assert_eq!(cache::range(&c, cache::RangeOp::CleanData, 0x7fff_ff00, 0x200), 0x7fff_ff20);
    assert_eq!(c.events(), vec![Cp15Event::Maintain(Op::CleanDataLine, 0x7fff_ff00)]);
    let c = Cp15Model::new(0, 0, &[]);
    assert_eq!(cache::range(&c, cache::RangeOp::InvalidateData, 0x100, 0xffff_ff00), 0x120);
    assert_eq!(c.events(), vec![Cp15Event::Maintain(Op::InvalidateDataLine, 0x100)]);
    let c = Cp15Model::new(0, 0, &[]);
    assert_eq!(cache::range(&c, cache::RangeOp::CleanData, 0x8000_0000, 0x40), 0x8000_0040);
    assert_eq!(
        c.events(),
        vec![Cp15Event::Maintain(Op::CleanDataLine, 0x8000_0000), Cp15Event::Maintain(Op::CleanDataLine, 0x8000_0020)]
    );
}

#[test]
fn cache_range_zero_start_or_length_invalidates_the_whole_cache() {
    for (start, length) in [(0, 0x40), (0x1000, 0), (0, 0)] {
        let c = Cp15Model::new(0, 0, &[]);
        assert_eq!(cache::range(&c, cache::RangeOp::InvalidateData, start, length), 0);
        assert_eq!(c.events(), vec![Cp15Event::Maintain(Op::InvalidateDataAll, 0)]);
        let c = Cp15Model::new(0, 0, &[]);
        assert_eq!(cache::range(&c, cache::RangeOp::InvalidateInstruction, start, length), 0);
        assert_eq!(c.events(), vec![Cp15Event::Maintain(Op::InvalidateInstructionAll, 0)]);
    }
}

/// LoC 2; level 1 split, level 2 unified.
const CLIDR: u32 = 0x0200_0023;
/// Level 1: 2 sets, 4 ways, 32-byte lines. Level 2: 2 sets, 1 way, 64-byte lines.
const CCSIDR: [(u32, u32); 2] = [(0, 0x2019), (2, 0x2002)];

fn set_way_walk(op: Op) -> Vec<Cp15Event> {
    let mut events = vec![Cp15Event::ReadClidr, Cp15Event::WriteCsselr(0), Cp15Event::Isb, Cp15Event::ReadCcsidr];
    for operand in [0xc000_0020, 0x8000_0020, 0x4000_0020, 0x0000_0020, 0xc000_0000, 0x8000_0000, 0x4000_0000, 0] {
        events.push(Cp15Event::Maintain(op, operand));
    }
    events.extend([Cp15Event::WriteCsselr(2), Cp15Event::Isb, Cp15Event::ReadCcsidr]);
    events.extend([Cp15Event::Maintain(op, 0x42), Cp15Event::Maintain(op, 0x02)]);
    events
}

#[test]
fn cache_whole_clean_walks_levels_then_sets_then_ways_descending() {
    let c = Cp15Model::new(0, CLIDR, &CCSIDR);
    assert_eq!(cache::range(&c, cache::RangeOp::CleanInvalidateData, 0, 0x40), CLIDR);
    assert_eq!(c.events(), set_way_walk(Op::CleanInvalidateDataSetWay));
    let c = Cp15Model::new(0, CLIDR, &CCSIDR);
    assert_eq!(cache::range(&c, cache::RangeOp::CleanData, 0x40, 0), CLIDR);
    assert_eq!(c.events(), set_way_walk(Op::CleanDataSetWay));
}

#[test]
fn cache_whole_clean_skips_levels_without_data_and_stops_at_loc() {
    let c = Cp15Model::new(0, 0x0300_0021, &[(2, 0x0000_2002)]);
    assert_eq!(cache::range(&c, cache::RangeOp::CleanData, 0, 0), 0x0300_0021);
    assert_eq!(
        c.events(),
        vec![
            Cp15Event::ReadClidr,
            Cp15Event::WriteCsselr(2),
            Cp15Event::Isb,
            Cp15Event::ReadCcsidr,
            Cp15Event::Maintain(Op::CleanDataSetWay, 0x42),
            Cp15Event::Maintain(Op::CleanDataSetWay, 0x02),
        ]
    );
    let c = Cp15Model::new(0, 0x0000_0023, &[]);
    assert_eq!(cache::range(&c, cache::RangeOp::CleanInvalidateData, 0, 0), 0x0000_0023);
    assert_eq!(c.events(), vec![Cp15Event::ReadClidr]);
}

const ACTIVE_A: u32 = 0x4005_4500;
const UNHANDLED_A: u32 = 0x4005_44fc;
const DESCRIPTOR: u32 = 0x0096_d000;

fn controller_a() -> controller::Controller {
    controller::Controller {
        table: table(TABLE_A),
        active: GuestAddress(ACTIVE_A),
        unhandled: GuestAddress(UNHANDLED_A),
    }
}

#[test]
fn initialize_resets_table_slots_and_the_executing_core_bank() {
    let m = Model::new(Bank::Secondary, 0x13);
    seed_other_bank(&m, Bank::Primary);
    m.set(UNHANDLED_A, 9);
    assert_eq!(controller::initialize(&m, controller_a()), 1);
    let secondary = Bank::Secondary.base();
    let events = m.events();
    assert_eq!(events.len(), 128 * 6 + 16 * 2 + 1 + 128 * 2 + 1 + 1);
    assert!(events.iter().all(|event| matches!(event, Event::Store(_, _))));
    assert_eq!(
        events[..6],
        [
            Event::Store(TABLE_A, DEFAULT_HANDLER),
            Event::Store(TABLE_A + 4, 0),
            Event::Store(TABLE_A + 8, 15),
            Event::Store(TABLE_A + 0xc, 0),
            Event::Store(TABLE_A + 0x10, 1),
            Event::Store(TABLE_A + 0x14, 0),
        ]
    );
    assert_eq!(events[128 * 6], Event::Store(ACTIVE_A, 0));
    assert_eq!(events[128 * 6 + 1], Event::Store(ACTIVE_A + 4, u32::MAX));
    assert_eq!(events[128 * 6 + 32], Event::Store(secondary + 0x1c0, 0xffff));
    assert_eq!(events[128 * 6 + 33], Event::Store(secondary + 0x400, DEFAULT_HANDLER));
    assert_eq!(events[128 * 6 + 34], Event::Store(secondary + 0x800, 15));
    assert_eq!(
        events[events.len() - 2..],
        [Event::Store(secondary + 0x1c4, 0xffff), Event::Store(UNHANDLED_A, 0)]
    );
    assert_eq!(m.get(TABLE_A + 127 * 24 + 0x10), 1);
    assert_eq!(m.get(ACTIVE_A + 15 * 8 + 4), u32::MAX);
    assert_eq!(m.get(secondary + 0x400 + 4 * 127), DEFAULT_HANDLER);
    assert_eq!(m.get(secondary + 0x800 + 4 * 127), 15);
    assert_eq!(bank_word(&m, Bank::Primary, VECTOR), 0x5a5a_5a5a);
    assert_eq!(m.cpsr(), 0x13);

    let m = Model::new(Bank::Primary, 0x13);
    assert_eq!(controller::initialize(&m, controller_a()), 0);
    assert_eq!(bank_word(&m, Bank::Primary, 0x1c4), 0xffff);
}

fn set_descriptor(m: &Model, words: [u32; 5]) {
    for (index, word) in words.into_iter().enumerate() {
        m.set(DESCRIPTOR + 4 * index as u32, word);
    }
}

#[test]
fn configure_validates_then_copies_each_word_under_the_irq_mask() {
    let m = Model::new(Bank::Primary, 0x13);
    set_descriptor(&m, [0x1234_5678, 1, 15, 1, 3]);
    let at = TABLE_A + 7 * 24;
    assert_eq!(controller::configure(&m, table(TABLE_A), 7, GuestAddress(DESCRIPTOR)), controller::Status::Ok);
    let mut expected: Vec<Event> = (0..5).map(|index| Event::Load(DESCRIPTOR + 4 * index)).collect();
    expected.push(Event::MaskIrq);
    for (index, word) in [0x1234_5678, 1, 15, 1, 3].into_iter().enumerate() {
        expected.push(Event::Load(DESCRIPTOR + 4 * index as u32));
        expected.push(Event::Store(at + 4 * index as u32, word));
    }
    expected.push(Event::RestoreIrq(0));
    assert_eq!(m.events(), expected);
    assert_eq!(m.cpsr(), 0x13);
}

#[test]
fn configure_refuses_at_the_first_invalid_word() {
    let cases: [([u32; 5], usize); 6] = [
        ([0, 0, 0, 0, 0], 1),
        ([1, 2, 0, 0, 0], 2),
        ([1, 0, 16, 0, 0], 3),
        ([1, 0, 0, 2, 0], 4),
        ([1, 0, 0, 0, 4], 5),
        ([1, 0, 0, 1, 4], 5),
    ];
    for (words, loads) in cases {
        let m = Model::new(Bank::Primary, 0x13);
        set_descriptor(&m, words);
        assert_eq!(
            controller::configure(&m, table(TABLE_A), 3, GuestAddress(DESCRIPTOR)),
            controller::Status::Refused,
            "{words:?}"
        );
        let expected: Vec<Event> = (0..loads as u32).map(|index| Event::Load(DESCRIPTOR + 4 * index)).collect();
        assert_eq!(m.events(), expected, "{words:?}");
    }
    let m = Model::new(Bank::Primary, 0x13);
    set_descriptor(&m, [1, 0, 0, 0, 0]);
    assert_eq!(controller::configure(&m, table(TABLE_A), 128, GuestAddress(DESCRIPTOR)), controller::Status::Refused);
    assert_eq!(m.events(), vec![]);
}

#[test]
fn configure_and_reset_index_below_the_table_for_a_negative_line() {
    let m = Model::new(Bank::Primary, 0x13);
    set_descriptor(&m, [0x1234_5678, 0, 2, 0, 1]);
    assert_eq!(controller::configure(&m, table(TABLE_A), -1, GuestAddress(DESCRIPTOR)), controller::Status::Ok);
    assert_eq!(m.get(TABLE_A - 24), 0x1234_5678);
    assert_eq!(m.get(TABLE_A - 24 + 8), 2);
    let m = Model::new(Bank::Primary, 0x13);
    assert_eq!(controller::reset(&m, table(TABLE_A), -2), controller::Status::Ok);
    assert_eq!(m.get(TABLE_A - 48), DEFAULT_HANDLER);
    assert_eq!(m.get(TABLE_A - 48 + 8), 15);
}

#[test]
fn reset_stores_the_default_registration_under_the_irq_mask() {
    let m = Model::new(Bank::Primary, 0x93);
    let at = TABLE_B + 9 * 24;
    assert_eq!(controller::reset(&m, table(TABLE_B), 9), controller::Status::Ok);
    assert_eq!(
        m.events(),
        vec![
            Event::MaskIrq,
            Event::Store(at, DEFAULT_HANDLER),
            Event::Store(at + 4, 0),
            Event::Store(at + 8, 15),
            Event::Store(at + 0xc, 0),
            Event::Store(at + 0x10, 1),
            Event::RestoreIrq(0x80),
        ]
    );
    assert_eq!(m.cpsr(), 0x93);
    let m = Model::new(Bank::Primary, 0x13);
    assert_eq!(controller::reset(&m, table(TABLE_B), 128), controller::Status::Refused);
    assert_eq!(m.events(), vec![]);
}

#[test]
fn handler_reads_the_entry_without_a_range_check() {
    let m = Model::new(Bank::Primary, 0x13);
    m.set(TABLE_A + 2 * 24, 0x0011_2233);
    m.set(TABLE_A + 200 * 24, 0x0044_5566);
    assert_eq!(controller::handler(&m, table(TABLE_A), 2), 0x0011_2233);
    assert_eq!(controller::handler(&m, table(TABLE_A), 200), 0x0044_5566);
    assert_eq!(m.events(), vec![Event::Load(TABLE_A + 48), Event::Load(TABLE_A + 4800)]);
}

#[test]
fn group_writes_select_the_bank_by_core_argument() {
    let secondary = Bank::Secondary.base();
    let primary = Bank::Primary.base();
    let m = Model::new(Bank::Primary, 0x13);
    assert_eq!(controller::set_enable(&m, controller::Core(2), 37), 2);
    assert_eq!(controller::raise(&m, controller::Core(0), 37), 0);
    assert_eq!(controller::clear(&m, controller::Core(1), 37), 4);
    assert_eq!(
        m.events(),
        vec![
            Event::Store(secondary + 0x84, 1 << 5),
            Event::Store(primary + 0xc4, 1 << 5),
            Event::Store(secondary + 0xa4, 1 << 5),
            Event::Store(secondary + 0xe4, 1 << 5),
        ]
    );
}

#[test]
fn clear_pending_uses_the_executing_core_bank() {
    let m = Model::new(Bank::Secondary, 0x13);
    assert_eq!(controller::clear_pending(&m, 100), 1);
    assert_eq!(m.events(), vec![Event::Store(Bank::Secondary.base() + 0xec, 1 << 4)]);
    let m = Model::new(Bank::Primary, 0x13);
    assert_eq!(controller::clear_pending(&m, 0), 0);
    assert_eq!(m.events(), vec![Event::Store(Bank::Primary.base() + 0xe0, 1)]);
}

#[test]
fn group_writes_follow_truncating_division_and_register_shift_for_odd_lines() {
    let primary = Bank::Primary.base();
    let m = Model::new(Bank::Primary, 0x13);
    controller::set_enable(&m, controller::Core(0), -1);
    controller::set_enable(&m, controller::Core(0), -32);
    controller::set_enable(&m, controller::Core(0), -33);
    controller::raise(&m, controller::Core(0), 160);
    assert_eq!(controller::clear(&m, controller::Core(0), -33), (-4i32) as u32);
    assert_eq!(
        m.events(),
        vec![
            Event::Store(primary + 0x80, 0),
            Event::Store(primary + 0x7c, 1),
            Event::Store(primary + 0x7c, 0),
            Event::Store(primary + 0xd4, 1),
            Event::Store(primary + 0x9c, 0),
            Event::Store(primary + 0xdc, 0),
        ]
    );
}

#[test]
fn copy_active_copies_sixteen_pairs_word_by_word() {
    let m = Model::new(Bank::Primary, 0x13);
    for slot in 0..16 {
        m.set(ACTIVE_A + 8 * slot, 0x100 + slot);
        m.set(ACTIVE_A + 8 * slot + 4, slot);
    }
    assert_eq!(controller::copy_active(&m, GuestAddress(ACTIVE_A), GuestAddress(DESCRIPTOR)), DESCRIPTOR);
    let events = m.events();
    assert_eq!(events.len(), 64);
    assert_eq!(
        events[..4],
        [
            Event::Load(ACTIVE_A),
            Event::Store(DESCRIPTOR, 0x100),
            Event::Load(ACTIVE_A + 4),
            Event::Store(DESCRIPTOR + 4, 0),
        ]
    );
    assert_eq!(m.get(DESCRIPTOR + 15 * 8), 0x10f);
    assert_eq!(m.get(DESCRIPTOR + 15 * 8 + 4), 15);
}

#[test]
fn core_index_follows_the_identity() {
    assert_eq!(controller::core_index(&Model::new(Bank::Primary, 0x13)), 0);
    assert_eq!(controller::core_index(&Model::new(Bank::Secondary, 0x13)), 1);
}
