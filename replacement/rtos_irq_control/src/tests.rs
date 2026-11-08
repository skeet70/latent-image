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
