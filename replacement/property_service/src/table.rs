//! Packed property and slot entries, in the layout `Encoding.hs` writes.

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Entry(pub u32);

/// The record branch, save policy and slot-1 default of one slot field.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Slot(pub u32);

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

/// Position of a slot field's entry in `SLOTS`.
#[derive(Clone, Copy)]
pub struct SlotIndex(pub u32);

/// The value the getter returns in default mode and the setter stores on a default request.
#[derive(Clone, Copy)]
pub struct FieldDefault(pub u32);

/// An unsigned bit range within a byte or halfword.
#[derive(Clone, Copy)]
pub struct Bits {
    pub width: Width,
    pub lsb: u32,
    pub count: u32,
}

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
}

#[derive(Clone, Copy)]
pub enum BankDefault {
    Constant(FieldDefault),
    TailByte(TailOffset),
}

/// The slot /= 1 branch, whose default is the same bits of the slot's 0x7cb804 record.
#[derive(Clone, Copy)]
pub struct SlotRecord {
    pub bits: Bits,
    pub offset: RecordOffset,
    pub save: Save,
    pub bank_default: BankDefault,
}

#[derive(Clone, Copy)]
pub enum Kind {
    /// Not reachable: the stock table still dispatches the property to its stock arm.
    Delegated,
    /// The stock fallback: get returns 0, set returns 0 and stores nothing.
    Unassigned,
    Field(Field),
    SlotField(SlotField),
}

fn bits(word: u32, half: u32, lsb: u32, count: u32) -> Bits {
    Bits {
        width: if (word >> half) & 1 != 0 { Width::Half } else { Width::Byte },
        lsb: (word >> lsb) & 0xf,
        count: (word >> count) & 0x1f,
    }
}

impl Entry {
    #[inline(always)]
    pub fn kind(self) -> Kind {
        let word = self.0;
        let offset = BankOffset((word >> 13) & 0x1ff);
        match word & 3 {
            0 => Kind::Delegated,
            1 => Kind::Unassigned,
            2 => Kind::Field(Field {
                bits: bits(word, 2, 4, 8),
                save: if word & (1 << 3) != 0 { Save::Bank0Previous } else { Save::None },
                offset,
                default: FieldDefault(word >> 22),
            }),
            _ => Kind::SlotField(SlotField { bits: bits(word, 2, 4, 8), offset, index: SlotIndex((word >> 22) & 0x7f) }),
        }
    }
}

impl Slot {
    #[inline(always)]
    pub fn record(self) -> SlotRecord {
        let word = self.0;
        let value = (word >> 18) & 0x3ff;
        SlotRecord {
            bits: bits(word, 6, 7, 11),
            offset: RecordOffset(word & 0x3f),
            save: if word & (1 << 16) != 0 { Save::Bank0Previous } else { Save::None },
            bank_default: if word & (1 << 17) != 0 { BankDefault::TailByte(TailOffset(value)) } else { BankDefault::Constant(FieldDefault(value)) },
        }
    }
}

impl Bits {
    pub fn mask(self) -> u32 {
        (1 << self.count) - 1
    }

    /// Whether the field fills its container.
    pub fn whole(self) -> bool {
        let container = match self.width {
            Width::Byte => 8,
            Width::Half => 16,
        };
        self.lsb == 0 && self.count == container
    }
}
