//! Packed property, slot and special entries, in the layout `Encoding.hs` writes.

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Entry(pub u32);

/// The record branch, save policy, slot-1 default and stock default of one slot field.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Slot(pub u32);

/// A field the property word cannot hold: its layout and constant, then its default source and hook.
#[derive(Clone, Copy)]
#[repr(C)]
pub struct Special(pub u32, pub u32);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Width {
    Byte,
    Half,
}

/// Whether a bank-0 store first copies the field's current bank-0 bits into 0x89ec00.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Save {
    Bank0Previous,
    None,
}

/// Offset of a field within one 0x1c2-byte bank image.
#[derive(Clone, Copy)]
pub struct BankOffset(pub u32);

/// Offset of a field within one 0x28-byte slot record.
#[derive(Clone, Copy)]
pub struct RecordOffset(pub u32);

/// Offset of a byte from the defaults table 0x81b074.
#[derive(Clone, Copy)]
pub struct TailOffset(pub u32);

/// Offset of a byte from 0x96b400.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct ConfigOffset(pub u32);

/// Position of a slot field's entry in `SLOTS`.
#[derive(Clone, Copy)]
pub struct SlotIndex(pub u32);

/// Position of a special field's entry in `SPECIALS`.
#[derive(Clone, Copy)]
pub struct SpecialIndex(pub u32);

/// The value the getter returns in default mode and the setter stores on a default request: a
/// constant, or the field's container at 0x81b074 + offset.
#[derive(Clone, Copy)]
pub enum FieldDefault {
    Constant(u32),
    TailByte(TailOffset),
    TailHalf(TailOffset),
}

/// 0x61d65c's argument.
#[derive(Clone, Copy)]
#[repr(u32)]
pub enum ProfileSelector {
    Two = 2,
    Three = 3,
    Four = 4,
}

/// The stock leaf whose result is a default.
#[derive(Clone, Copy)]
pub enum StockDefault {
    Stock61df44,
    Stock61d8a4,
    Stock61d59c,
    Stock61d618,
    Stock61d5f0,
    Stock61d65c(ProfileSelector),
}

/// A special field's default: a constant, or a stock leaf's result or a configuration byte, each
/// with the provisional value a default request stores first.
#[derive(Clone, Copy)]
pub enum Source {
    Constant(u32),
    Stock(StockDefault, u32),
    Config(ConfigOffset, u32),
}

/// What surrounds or follows a set's save and store.
#[derive(Clone, Copy)]
pub enum Hook {
    None,
    /// On bank 0 the setter stub calls 0x61da3c before and 0x61d944 after them.
    Bank0Bracket,
    /// Repeats them when the byte at 0x96b400 + offset is 0.
    GatedResave(ConfigOffset),
}

/// An unsigned bit range within a byte or halfword: bits 0-3 lsb, 4-8 width in bits, 9 halfword.
/// Packed so it passes in a register: aggregates the compiler builds on the stack can be stored
/// through a register other than sp, which admission refuses.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Bits(u32);

#[derive(Clone, Copy)]
pub struct Field {
    pub bits: Bits,
    pub save: Save,
    pub offset: BankOffset,
    pub default: FieldDefault,
}

/// The slot == 1 branch of a slot field and its entry in `SLOTS`.
#[derive(Clone, Copy)]
pub struct SlotField {
    pub bits: Bits,
    pub offset: BankOffset,
    pub index: SlotIndex,
    /// Every read and write also goes to slot record 0.
    pub mirror: bool,
}

#[derive(Clone, Copy)]
pub enum BankDefault {
    Constant(u32),
    TailByte(TailOffset),
}

/// The default of both branches: the bank default and the slot-defaults record, or a stock leaf's
/// result with those two as provisional values.
#[derive(Clone, Copy)]
pub enum SlotDefault {
    Record,
    Stock(StockDefault),
}

/// The slot /= 1 branch, whose stage-2 default is the same bits of the slot's 0x7cb804 record.
#[derive(Clone, Copy)]
pub struct SlotRecord {
    pub bits: Bits,
    pub offset: RecordOffset,
    pub save: Save,
    pub bank_default: BankDefault,
    pub default: SlotDefault,
}

#[derive(Clone, Copy)]
pub struct HookedField {
    pub bits: Bits,
    pub save: Save,
    pub offset: BankOffset,
    pub source: Source,
    pub hook: Hook,
}

/// A field whose set is the stock setter fallback: it returns 0 and stores nothing.
#[derive(Clone, Copy)]
pub struct ReadOnlyField {
    pub bits: Bits,
    pub offset: BankOffset,
    pub default: u32,
}

/// Two whole bytes a set stores with one value; on bank 0 the setter stub's bracket calls surround
/// the first. Get returns 0.
#[derive(Clone, Copy)]
pub struct WriteOnlyPair {
    pub bits: Bits,
    pub save: Save,
    pub first: BankOffset,
    pub second: BankOffset,
    pub default: u32,
}

#[derive(Clone, Copy)]
pub enum SpecialField {
    Hooked(HookedField),
    ReadOnly(ReadOnlyField),
    WriteOnlyPair(WriteOnlyPair),
}

#[derive(Clone, Copy)]
pub enum Kind {
    /// The stock fallback: get returns 0, set returns 0 and stores nothing.
    Unassigned,
    Field(Field),
    SlotField(SlotField),
    Special(SpecialIndex),
}

fn bits(word: u32, half: u32, lsb: u32, count: u32) -> Bits {
    Bits(((word >> lsb) & 0xf) | (((word >> count) & 0x1f) << 4) | (((word >> half) & 1) << 9))
}

fn save(word: u32, bit: u32) -> Save {
    if (word >> bit) & 1 != 0 {
        Save::Bank0Previous
    } else {
        Save::None
    }
}

/// The leaf a stock code names.
fn stock(code: u32) -> Option<StockDefault> {
    match code {
        1 => Some(StockDefault::Stock61df44),
        2 => Some(StockDefault::Stock61d8a4),
        3 => Some(StockDefault::Stock61d59c),
        4 => Some(StockDefault::Stock61d618),
        5 => Some(StockDefault::Stock61d5f0),
        6 => Some(StockDefault::Stock61d65c(ProfileSelector::Two)),
        7 => Some(StockDefault::Stock61d65c(ProfileSelector::Three)),
        8 => Some(StockDefault::Stock61d65c(ProfileSelector::Four)),
        _ => None,
    }
}

impl Entry {
    /// The property's kind, or None for a kind-0 word with index 0.
    #[inline(always)]
    pub fn kind(self) -> Option<Kind> {
        let word = self.0;
        let offset = BankOffset((word >> 13) & 0x1ff);
        Some(match word & 3 {
            0 => match (word >> 2) & 0x3f {
                0 => return None,
                index => Kind::Special(SpecialIndex(index - 1)),
            },
            1 => Kind::Unassigned,
            2 => Kind::Field(Field {
                bits: bits(word, 2, 4, 8),
                save: save(word, 3),
                offset,
                default: match (word >> 31, (word >> 2) & 1) {
                    (0, _) => FieldDefault::Constant((word >> 22) & 0x1ff),
                    (_, 0) => FieldDefault::TailByte(TailOffset((word >> 22) & 0xff)),
                    _ => FieldDefault::TailHalf(TailOffset((word >> 22) & 0xff)),
                },
            }),
            _ => Kind::SlotField(SlotField {
                bits: bits(word, 2, 4, 8),
                offset,
                index: SlotIndex((word >> 22) & 0x7f),
                mirror: (word >> 29) & 1 != 0,
            }),
        })
    }
}

impl Slot {
    /// The record branch, or None for an unassigned stock code.
    #[inline(always)]
    pub fn record(self) -> Option<SlotRecord> {
        let word = self.0;
        let value = (word >> 18) & 0x3ff;
        let default = match word >> 28 {
            0 => SlotDefault::Record,
            code => SlotDefault::Stock(stock(code)?),
        };
        Some(SlotRecord {
            bits: bits(word, 6, 7, 11),
            offset: RecordOffset(word & 0x3f),
            save: save(word, 16),
            bank_default: if word & (1 << 17) != 0 { BankDefault::TailByte(TailOffset(value)) } else { BankDefault::Constant(value) },
            default,
        })
    }
}

impl Special {
    /// The field, or None for an unassigned subkind, source or hook.
    #[inline(always)]
    pub fn field(self) -> Option<SpecialField> {
        let Special(layout, behaviour) = self;
        let bits = bits(layout, 2, 4, 8);
        let offset = BankOffset((layout >> 13) & 0x1ff);
        let value = (layout >> 22) & 0x1ff;
        match layout & 3 {
            0 => Some(SpecialField::Hooked(HookedField {
                bits,
                save: save(layout, 3),
                offset,
                source: match behaviour & 3 {
                    0 => Source::Constant(value),
                    1 => Source::Stock(stock((behaviour >> 2) & 0xf)?, value),
                    2 => Source::Config(ConfigOffset((behaviour >> 6) & 0x1ff), value),
                    _ => return None,
                },
                hook: match (behaviour >> 15) & 3 {
                    0 => Hook::None,
                    1 => Hook::Bank0Bracket,
                    2 => Hook::GatedResave(ConfigOffset((behaviour >> 17) & 0x1ff)),
                    _ => return None,
                },
            })),
            1 => Some(SpecialField::ReadOnly(ReadOnlyField { bits, offset, default: value })),
            2 => match (behaviour & 3, (behaviour >> 15) & 3) {
                (0, 1) => Some(SpecialField::WriteOnlyPair(WriteOnlyPair {
                    bits,
                    save: save(layout, 3),
                    first: offset,
                    second: BankOffset((behaviour >> 17) & 0x1ff),
                    default: value,
                })),
                _ => None,
            },
            _ => None,
        }
    }
}

impl Bits {
    pub fn width(self) -> Width {
        if (self.0 >> 9) & 1 != 0 {
            Width::Half
        } else {
            Width::Byte
        }
    }

    pub fn lsb(self) -> u32 {
        self.0 & 0xf
    }

    pub fn count(self) -> u32 {
        (self.0 >> 4) & 0x1f
    }

    pub fn mask(self) -> u32 {
        (1 << self.count()) - 1
    }

    /// Whether the field fills its container.
    pub fn whole(self) -> bool {
        let container = match self.width() {
            Width::Byte => 8,
            Width::Half => 16,
        };
        self.lsb() == 0 && self.count() == container
    }
}
