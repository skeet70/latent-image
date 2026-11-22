//! The stock controller entries in `0x42854..0x434f0` beside enable, disable and define.
//! Lines arrive as signed words; entries without a range check index by stock's
//! truncating division, so negative and large lines address outside the table and bank.

use super::layout::GuestAddress;
use super::{
    shift_left_register, Backend, Bank, Table, DEFAULT_MODE, DEFAULT_PRIORITY, ENTRY_SIZE,
    INTERRUPT_COUNT,
};

/// `{word, line}` pairs.
pub const ACTIVE_SLOTS: u32 = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Controller {
    pub table: Table,
    pub active: GuestAddress,
    pub unhandled: GuestAddress,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(i32)]
pub enum Status {
    Ok = 0,
    Refused = -1,
}

/// A caller-supplied core number: zero selects the primary bank.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Core(pub u32);

impl Core {
    const fn bank(self) -> Bank {
        if self.0 == 0 {
            Bank::Primary
        } else {
            Bank::Secondary
        }
    }
}

/// A line as stock's `asr`/`and`/`rsb` sequence splits it.
#[derive(Clone, Copy)]
struct RawGroup {
    group: i32,
    bit: u32,
}

impl RawGroup {
    #[inline(always)]
    const fn of(line: i32) -> Self {
        Self {
            group: line / 32,
            bit: shift_left_register(1, (line % 32) as u32),
        }
    }

    #[inline(always)]
    const fn word(self, bank: Bank, offset: u32) -> GuestAddress {
        GuestAddress(
            bank.base()
                .wrapping_add(offset)
                .wrapping_add((self.group as u32).wrapping_mul(4)),
        )
    }
}

#[inline(always)]
fn raw_entry(table: Table, line: i32) -> GuestAddress {
    GuestAddress(table.base().0.wrapping_add((line as u32).wrapping_mul(ENTRY_SIZE)))
}

#[inline(always)]
const fn core_number(bank: Bank) -> u32 {
    match bank {
        Bank::Primary => 0,
        Bank::Secondary => 1,
    }
}

/// Stock `0x42854`.
#[inline(always)]
pub fn core_index<B: Backend>(m: &B) -> u32 {
    core_number(m.bank())
}

/// Stock `0x4286c`. Returns the core index.
#[inline(always)]
pub fn initialize<B: Backend>(m: &B, c: Controller) -> u32 {
    let bank = m.bank();
    let marker = c.table.default_handler().get();
    let mut line = 0;
    while line < INTERRUPT_COUNT {
        let entry = c.table.base().offset(line * ENTRY_SIZE);
        m.store_word(entry, marker);
        m.store_word(entry.offset(0x04), 0);
        m.store_word(entry.offset(0x08), DEFAULT_PRIORITY);
        m.store_word(entry.offset(0x0c), 0);
        m.store_word(entry.offset(0x10), DEFAULT_MODE);
        m.store_word(entry.offset(0x14), 0);
        line += 1;
    }
    let mut slot = 0;
    while slot < ACTIVE_SLOTS {
        m.store_word(c.active.offset(8 * slot), 0);
        m.store_word(c.active.offset(8 * slot + 4), u32::MAX);
        slot += 1;
    }
    let base = GuestAddress(bank.base());
    m.store_word(base.offset(0x1c0), 0xffff);
    let mut line = 0;
    while line < INTERRUPT_COUNT {
        m.store_word(base.offset(0x400 + 4 * line), marker);
        m.store_word(base.offset(0x800 + 4 * line), DEFAULT_PRIORITY);
        line += 1;
    }
    m.store_word(base.offset(0x1c4), 0xffff);
    m.store_word(c.unhandled, 0);
    core_number(bank)
}

/// Stock `0x429ac`: validates the five descriptor words, then copies them under the IRQ
/// mask, re-reading each.
#[inline(always)]
pub fn configure<B: Backend>(m: &B, table: Table, line: i32, descriptor: GuestAddress) -> Status {
    if line >= INTERRUPT_COUNT as i32 {
        return Status::Refused;
    }
    if m.load_word(descriptor) == 0
        || m.load_word(descriptor.offset(0x04)) > 1
        || m.load_word(descriptor.offset(0x08)) > 15
        || m.load_word(descriptor.offset(0x0c)) > 1
        || m.load_word(descriptor.offset(0x10)) > 3
    {
        return Status::Refused;
    }
    let saved = m.mask_irq();
    let entry = raw_entry(table, line);
    let mut offset = 0;
    while offset <= 0x10 {
        m.store_word(entry.offset(offset), m.load_word(descriptor.offset(offset)));
        offset += 4;
    }
    m.restore_irq(saved);
    Status::Ok
}

/// Stock `0x42a98`: configure with the default registration.
#[inline(always)]
pub fn reset<B: Backend>(m: &B, table: Table, line: i32) -> Status {
    if line >= INTERRUPT_COUNT as i32 {
        return Status::Refused;
    }
    let saved = m.mask_irq();
    let entry = raw_entry(table, line);
    m.store_word(entry, table.default_handler().get());
    m.store_word(entry.offset(0x04), 0);
    m.store_word(entry.offset(0x08), DEFAULT_PRIORITY);
    m.store_word(entry.offset(0x0c), 0);
    m.store_word(entry.offset(0x10), DEFAULT_MODE);
    m.restore_irq(saved);
    Status::Ok
}

/// Stock `0x42adc`, unchecked.
#[inline(always)]
pub fn handler<B: Backend>(m: &B, table: Table, line: i32) -> u32 {
    m.load_word(raw_entry(table, line))
}

/// Stock `0x432f8`: `+0x80`. Returns `core`.
#[inline(always)]
pub fn set_enable<B: Backend>(m: &B, core: Core, line: i32) -> u32 {
    let group = RawGroup::of(line);
    m.store_word(group.word(core.bank(), 0x80), group.bit);
    core.0
}

/// Stock `0x43358`: `+0xa0` then `+0xe0`. Returns `4 * group`.
#[inline(always)]
pub fn clear<B: Backend>(m: &B, core: Core, line: i32) -> u32 {
    let group = RawGroup::of(line);
    m.store_word(group.word(core.bank(), 0xa0), group.bit);
    m.store_word(group.word(core.bank(), 0xe0), group.bit);
    (group.group as u32).wrapping_mul(4)
}

/// Stock `0x433e4`: `+0xc0`. Returns `core`.
#[inline(always)]
pub fn raise<B: Backend>(m: &B, core: Core, line: i32) -> u32 {
    let group = RawGroup::of(line);
    m.store_word(group.word(core.bank(), 0xc0), group.bit);
    core.0
}

/// Stock `0x43444`: the executing core's `+0xe0`. Returns the core index.
#[inline(always)]
pub fn clear_pending<B: Backend>(m: &B, line: i32) -> u32 {
    let bank = m.bank();
    let group = RawGroup::of(line);
    m.store_word(group.word(bank, 0xe0), group.bit);
    core_number(bank)
}

/// Stock `0x434a4`. Returns `buffer`.
#[inline(always)]
pub fn copy_active<B: Backend>(m: &B, active: GuestAddress, buffer: GuestAddress) -> u32 {
    let mut slot = 0;
    while slot < ACTIVE_SLOTS {
        m.store_word(buffer.offset(8 * slot), m.load_word(active.offset(8 * slot)));
        m.store_word(buffer.offset(8 * slot + 4), m.load_word(active.offset(8 * slot + 4)));
        slot += 1;
    }
    buffer.0
}
