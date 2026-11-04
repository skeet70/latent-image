//! One packed property entry, in the layout `Encoding.hs` writes.

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Entry(pub u32);

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

/// The value the getter returns in default mode and the setter stores on a default request.
#[derive(Clone, Copy)]
pub struct FieldDefault(pub u32);

#[derive(Clone, Copy)]
pub struct Field {
    pub width: Width,
    pub save: Save,
    pub lsb: u32,
    pub bits: u32,
    pub offset: BankOffset,
    pub default: FieldDefault,
}

#[derive(Clone, Copy)]
pub enum Kind {
    /// Not reachable: the stock table still dispatches the property to its stock arm.
    Delegated,
    /// The stock fallback: get returns 0, set returns 0 and stores nothing.
    Unassigned,
    Field(Field),
}

impl Entry {
    #[inline(always)]
    pub fn kind(self) -> Kind {
        let word = self.0;
        match word & 3 {
            1 => Kind::Unassigned,
            2 => Kind::Field(Field {
                width: if word & (1 << 2) != 0 { Width::Half } else { Width::Byte },
                save: if word & (1 << 3) != 0 { Save::Bank0Previous } else { Save::None },
                lsb: (word >> 4) & 0xf,
                bits: (word >> 8) & 0x1f,
                offset: BankOffset((word >> 13) & 0x1ff),
                default: FieldDefault(word >> 22),
            }),
            _ => Kind::Delegated,
        }
    }
}

impl Field {
    pub fn mask(self) -> u32 {
        (1 << self.bits) - 1
    }

    /// Whether the field fills its container.
    pub fn whole(self) -> bool {
        let container = match self.width {
            Width::Byte => 8,
            Width::Half => 16,
        };
        self.lsb == 0 && self.bits == container
    }
}
