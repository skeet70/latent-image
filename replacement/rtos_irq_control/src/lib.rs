//! IRQ enable, disable and handler definition for the X-M1 1.50 root slots `+0x158`,
//! `+0x15c` and `+0x160`. The slot's loader domain selects the 128-entry registration
//! table; the executing core selects the controller bank.

#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

#[cfg(test)]
mod tests;

use core::num::NonZeroU32;
use layout::GuestAddress;

pub const INTERRUPT_COUNT: u32 = 128;
pub const ENTRY_SIZE: u32 = 0x18;
pub const DEFAULT_PRIORITY: u32 = 15;
pub const DEFAULT_MODE: u32 = 1;

/// Controller byte offsets; the contract lists the same words as `+0x20`, `+0x28`, ...
const GROUP_ENABLE: u32 = 0x80;
const GROUP_CLEAR: u32 = 0xa0;
const GROUP_CONFIG_60: u32 = 0x60;
const GROUP_CONFIG_100: u32 = 0x100;
const GROUP_SET_PENDING: u32 = 0x120;
const GROUP_CLEAR_PENDING: u32 = 0xe0;
const HALF_140: u32 = 0x140;
const HALF_180: u32 = 0x180;
const VECTOR: u32 = 0x400;
const PRIORITY: u32 = 0x800;

/// The I and F bits returned by one of the stock mask helpers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct SavedInterrupts(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bank {
    Primary,
    Secondary,
}

impl Bank {
    pub const fn base(self) -> u32 {
        match self {
            Bank::Primary => 0xfffb_3000,
            Bank::Secondary => 0xfffb_4000,
        }
    }
}

pub trait Backend {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
    /// `0x42574`: sets I, returns the prior I bit.
    fn mask_irq(&self) -> SavedInterrupts;
    /// `0x4257c`: restores I from `saved`, preserving the other control bits.
    fn restore_irq(&self, saved: SavedInterrupts);
    /// `0x42560`.
    fn mask_irq_fiq(&self) -> SavedInterrupts;
    /// `0x42568`: restores I and F from `saved`, preserving the other control bits.
    fn restore_irq_fiq(&self, saved: SavedInterrupts);
    /// `0x42854`: 0 for CP14 identity `0x4003`, otherwise 1.
    fn bank(&self) -> Bank;
}

/// 0..=127.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct InterruptLine(u8);

impl InterruptLine {
    pub const fn new(value: u32) -> Option<Self> {
        if value < INTERRUPT_COUNT {
            Some(Self(value as u8))
        } else {
            None
        }
    }

    pub const fn from_static(value: u32) -> Self {
        match Self::new(value) {
            Some(line) => line,
            None => panic!("interrupt line out of range"),
        }
    }

    /// Enable and disable compare only `raw > 127`; a negative value indexes below the
    /// registration table.
    pub const fn from_raw(raw: i32) -> Option<Self> {
        if raw < 0 {
            None
        } else {
            Self::new(raw as u32)
        }
    }

    /// Define truncates its first argument with `uxth`.
    pub const fn from_define_argument(raw: u32) -> Option<Self> {
        Self::new(raw & 0xffff)
    }

    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Handler(NonZeroU32);

impl Handler {
    pub const fn new(value: u32) -> Option<Self> {
        match NonZeroU32::new(value) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn from_static(value: u32) -> Self {
        match Self::new(value) {
            Some(handler) => handler,
            None => panic!("handler is zero"),
        }
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Priority(u8);

impl Priority {
    pub const fn new(value: u32) -> Option<Self> {
        if value < 16 {
            Some(Self(value as u8))
        } else {
            None
        }
    }

    pub const fn from_static(value: u32) -> Self {
        match Self::new(value) {
            Some(priority) => priority,
            None => panic!("priority out of range"),
        }
    }

    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Flag(u32);

impl Flag {
    pub const fn new(value: u32) -> Option<Self> {
        if value <= 1 {
            Some(Self(value))
        } else {
            None
        }
    }

    pub const fn from_static(value: u32) -> Self {
        match Self::new(value) {
            Some(flag) => flag,
            None => panic!("flag out of range"),
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(transparent)]
pub struct Mode(u8);

impl Mode {
    pub const fn new(value: u32) -> Option<Self> {
        if value <= 3 {
            Some(Self(value as u8))
        } else {
            None
        }
    }

    pub const fn from_static(value: u32) -> Self {
        match Self::new(value) {
            Some(mode) => mode,
            None => panic!("mode out of range"),
        }
    }

    pub const fn get(self) -> u32 {
        self.0 as u32
    }
}

/// The validated `+0x160` packet fields; `+0` is never read and `+4` is written as zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Registration {
    pub handler: Handler,
    pub priority: Priority,
    pub flag_c: Flag,
    pub mode: Mode,
}

impl Registration {
    pub const fn new(handler: u32, priority: u32, flag_c: u32, mode: u32) -> Option<Self> {
        match (
            Handler::new(handler),
            Priority::new(priority),
            Flag::new(flag_c),
            Mode::new(mode),
        ) {
            (Some(handler), Some(priority), Some(flag_c), Some(mode)) => Some(Self {
                handler,
                priority,
                flag_c,
                mode,
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Table {
    base: GuestAddress,
    default_handler: Handler,
}

impl Table {
    pub const fn new(base: GuestAddress, default_handler: Handler) -> Self {
        Self {
            base,
            default_handler,
        }
    }

    pub const fn base(self) -> GuestAddress {
        self.base
    }

    pub const fn default_handler(self) -> Handler {
        self.default_handler
    }

    pub const fn entry(self, line: InterruptLine) -> GuestAddress {
        self.base.offset(line.get() * ENTRY_SIZE)
    }

    #[inline(always)]
    fn load<B: Backend>(self, m: &B, line: InterruptLine, offset: u32) -> u32 {
        m.load_word(self.entry(line).offset(offset))
    }

    #[inline(always)]
    fn store<B: Backend>(self, m: &B, line: InterruptLine, offset: u32, value: u32) {
        m.store_word(self.entry(line).offset(offset), value);
    }

    #[inline(always)]
    fn store_entry<B: Backend>(self, m: &B, line: InterruptLine, registration: Registration) {
        self.store(m, line, 0x00, registration.handler.get());
        self.store(m, line, 0x04, 0);
        self.store(m, line, 0x08, registration.priority.get());
        self.store(m, line, 0x0c, registration.flag_c.get());
        self.store(m, line, 0x10, registration.mode.get());
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrqError {
    OutOfRange,
    NoHandler,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefineOutcome {
    Ok,
    Refused,
}

#[derive(Clone, Copy)]
struct Group {
    base: u32,
    index: u32,
}

impl Group {
    #[inline(always)]
    fn enable(self) -> GuestAddress {
        GuestAddress(self.base + GROUP_ENABLE + 4 * self.index)
    }

    #[inline(always)]
    fn clear(self) -> GuestAddress {
        GuestAddress(self.base + GROUP_CLEAR + 4 * self.index)
    }

    #[inline(always)]
    fn config_60(self) -> GuestAddress {
        GuestAddress(self.base + GROUP_CONFIG_60 + 4 * self.index)
    }

    #[inline(always)]
    fn config_100(self) -> GuestAddress {
        GuestAddress(self.base + GROUP_CONFIG_100 + 4 * self.index)
    }

    #[inline(always)]
    fn set_pending(self) -> GuestAddress {
        GuestAddress(self.base + GROUP_SET_PENDING + 4 * self.index)
    }

    #[inline(always)]
    fn clear_pending(self) -> GuestAddress {
        GuestAddress(self.base + GROUP_CLEAR_PENDING + 4 * self.index)
    }
}

#[derive(Clone, Copy)]
struct BankWords {
    base: u32,
}

impl BankWords {
    #[inline(always)]
    fn new(bank: Bank) -> Self {
        Self { base: bank.base() }
    }

    #[inline(always)]
    fn group(self, index: u32) -> Group {
        Group {
            base: self.base,
            index,
        }
    }

    #[inline(always)]
    fn half_140(self, pair: u32) -> GuestAddress {
        GuestAddress(self.base + HALF_140 + 4 * pair)
    }

    #[inline(always)]
    fn half_180(self, pair: u32) -> GuestAddress {
        GuestAddress(self.base + HALF_180 + 4 * pair)
    }

    #[inline(always)]
    fn vector(self, line: InterruptLine) -> GuestAddress {
        GuestAddress(self.base + VECTOR + 4 * line.get())
    }

    #[inline(always)]
    fn priority(self, line: InterruptLine) -> GuestAddress {
        GuestAddress(self.base + PRIORITY + 4 * line.get())
    }
}

#[inline(always)]
fn read_definition<B: Backend>(m: &B, packet: GuestAddress) -> Option<Registration> {
    let handler = m.load_word(packet.offset(0x04));
    let priority = m.load_word(packet.offset(0x08));
    let flag_c = m.load_word(packet.offset(0x0c));
    let mode = m.load_word(packet.offset(0x10));
    Registration::new(handler, priority, flag_c, mode)
}

#[inline(always)]
fn configure<B: Backend>(m: &B, table: Table, line: InterruptLine, registration: Registration) {
    let saved = m.mask_irq();
    table.store_entry(m, line, registration);
    m.restore_irq(saved);
}

#[inline(always)]
fn reset<B: Backend>(m: &B, table: Table, line: Option<InterruptLine>) {
    if let Some(line) = line {
        let registration = Registration {
            handler: table.default_handler(),
            priority: Priority::from_static(DEFAULT_PRIORITY),
            flag_c: Flag::from_static(0),
            mode: Mode::from_static(DEFAULT_MODE),
        };
        configure(m, table, line, registration);
    }
}

#[inline(always)]
pub fn enable<B: Backend>(m: &B, table: Table, line: InterruptLine) -> Result<(), IrqError> {
    let bank = BankWords::new(m.bank());
    let saved = m.mask_irq();
    if table.load(m, line, 0x00) == table.default_handler().get() {
        m.restore_irq(saved);
        return Err(IrqError::NoHandler);
    }

    let index = line.get() / 32;
    let bit = 1u32 << (line.get() % 32);
    let pair = line.get() / 16;
    let field = 3u32 << (2 * (line.get() % 16));
    let bit_shift = line.get() % 32;
    let field_shift = 2 * (line.get() % 16);
    let group = bank.group(index);

    let enable_word = m.load_word(group.enable());
    m.store_word(group.clear(), !0);

    let config_60 = m.load_word(group.config_60());
    let word4 = table.load(m, line, 0x04);
    m.store_word(group.config_60(), (config_60 & !bit) | (word4 << bit_shift));

    let config_100 = m.load_word(group.config_100());
    let word_c = table.load(m, line, 0x0c);
    m.store_word(group.config_100(), (config_100 & !bit) | (word_c << bit_shift));

    let word_c_again = table.load(m, line, 0x0c);
    if word_c_again == 0 {
        let half_180 = m.load_word(bank.half_180(pair));
        let mode = table.load(m, line, 0x10);
        m.store_word(bank.half_180(pair), (half_180 & !field) | (mode << field_shift));
        let half_140 = m.load_word(bank.half_140(pair));
        m.store_word(bank.half_140(pair), half_140 & !field);
    } else {
        let half_180 = m.load_word(bank.half_180(pair));
        m.store_word(bank.half_180(pair), half_180 & !field);
        let half_140 = m.load_word(bank.half_140(pair));
        let mode = table.load(m, line, 0x10);
        m.store_word(bank.half_140(pair), (half_140 & !field) | (mode << field_shift));
    }

    let published_handler = table.load(m, line, 0x00);
    m.store_word(bank.vector(line), published_handler);
    let priority = table.load(m, line, 0x08);
    m.store_word(bank.priority(line), priority);
    m.store_word(group.set_pending(), bit);
    m.store_word(group.enable(), enable_word | bit);
    m.restore_irq(saved);
    Ok(())
}

#[inline(always)]
pub fn enable_raw<B: Backend>(m: &B, table: Table, raw: i32) -> Result<(), IrqError> {
    match InterruptLine::from_raw(raw) {
        Some(line) => enable(m, table, line),
        None => Err(IrqError::OutOfRange),
    }
}

#[inline(always)]
pub fn disable<B: Backend>(m: &B, table: Table, line: InterruptLine) -> Result<(), IrqError> {
    let bank = BankWords::new(m.bank());
    let saved = m.mask_irq();
    let index = line.get() / 32;
    let bit = 1u32 << (line.get() % 32);
    let pair = line.get() / 16;
    let field = 3u32 << (2 * (line.get() % 16));
    let group = bank.group(index);

    let enable_word = m.load_word(group.enable());
    m.store_word(group.clear(), !0);
    let config_60 = m.load_word(group.config_60());
    m.store_word(group.config_60(), config_60 & !bit);
    m.store_word(group.clear_pending(), bit);
    let config_100 = m.load_word(group.config_100());
    m.store_word(group.config_100(), config_100 & !bit);
    m.store_word(group.set_pending(), bit);
    let half_180 = m.load_word(bank.half_180(pair));
    m.store_word(bank.half_180(pair), half_180 & !field);
    let half_140 = m.load_word(bank.half_140(pair));
    m.store_word(bank.half_140(pair), half_140 & !field);
    m.store_word(bank.vector(line), table.default_handler().get());
    m.store_word(group.enable(), enable_word & !bit);
    m.restore_irq(saved);
    Ok(())
}

#[inline(always)]
pub fn disable_raw<B: Backend>(m: &B, table: Table, raw: i32) -> Result<(), IrqError> {
    match InterruptLine::from_raw(raw) {
        Some(line) => disable(m, table, line),
        None => Err(IrqError::OutOfRange),
    }
}

#[inline(always)]
pub fn define<B: Backend>(
    m: &B,
    table: Table,
    raw_line: u32,
    packet: Option<GuestAddress>,
) -> DefineOutcome {
    let saved = m.mask_irq_fiq();
    let line = InterruptLine::from_define_argument(raw_line);
    let outcome = match packet {
        None => {
            reset(m, table, line);
            DefineOutcome::Ok
        }
        Some(packet) => {
            let registration = read_definition(m, packet);
            match (line, registration) {
                (Some(line), Some(registration)) => {
                    configure(m, table, line, registration);
                    DefineOutcome::Ok
                }
                _ => DefineOutcome::Refused,
            }
        }
    };
    m.restore_irq_fiq(saved);
    outcome
}

#[cfg(all(target_arch = "arm", not(test), not(feature = "stage")))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
