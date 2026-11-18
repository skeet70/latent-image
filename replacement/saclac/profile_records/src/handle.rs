#![forbid(unsafe_code)]

//! Profile-record handles: which registry slot a handle names and what it may touch.

/// One of the registry's sixteen handle slots.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slot(u8);

impl Slot {
    pub const COUNT: usize = 16;

    pub const fn new(slot: u8) -> Option<Self> {
        if (slot as usize) < Self::COUNT { Some(Self(slot)) } else { None }
    }

    pub const fn index(self) -> usize { self.0 as usize }
}

/// The generation a slot was opened with; a handle names its slot only while the two agree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Serial(u16);

impl Serial {
    pub const MAX: u16 = 0xfff;

    pub const fn new(serial: u16) -> Option<Self> {
        if serial <= Self::MAX { Some(Self(serial)) } else { None }
    }

    pub const fn get(self) -> u16 { self.0 }
}

/// One of the 43 groups of fields a slot grants read and write access to separately.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct AccessGroup(u8);

impl AccessGroup {
    pub const COUNT: usize = 43;

    pub const fn new(group: u8) -> Option<Self> {
        if (group as usize) < Self::COUNT { Some(Self(group)) } else { None }
    }

    /// For catalog constants: a group out of range fails const evaluation.
    pub(crate) const fn at(group: u8) -> Self {
        match Self::new(group) {
            Some(group) => group,
            None => panic!("access group out of range"),
        }
    }

    pub const fn index(self) -> usize { self.0 as usize }
}

/// The groups a handle reaches: every group, or the one it was restricted to.
///
/// A scope word between 43 and 62 admits no group; stock never issues one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Scope(u8);

impl Scope {
    pub(crate) const ALL_WORD: u8 = 63;
    pub const ALL: Scope = Scope(Self::ALL_WORD);

    pub const fn one(group: AccessGroup) -> Self { Self(group.0) }

    pub(crate) const fn from_word(word: u8) -> Self { Self(word & Self::ALL_WORD) }

    pub(crate) const fn word(self) -> u8 { self.0 }

    pub const fn admits(self, group: AccessGroup) -> bool { self.0 == Self::ALL_WORD || self.0 == group.0 }

    const fn is_all(self) -> bool { self.0 == Self::ALL_WORD }
}

/// A caller's byte in the handle; stock forms it from two nibbles.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Tag(pub u8);

impl Tag {
    pub const fn from_nibbles(high: u8, low: u8) -> Self { Self((high << 4) | (low & 0xf)) }
}

/// The access a derived handle asks for. Only `write` is carried by the handle; read access
/// comes from the slot's grants alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    None,
    Read,
    Write,
    ReadWrite,
}

impl Access {
    const fn bits(self) -> u8 {
        match self {
            Access::None => 0,
            Access::Read => 1,
            Access::Write => 2,
            Access::ReadWrite => 3,
        }
    }

    const fn writes(self) -> bool { self.bits() & 2 != 0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeriveRefused {
    /// A restricted handle derives only when the request's access bits equal its group's number
    /// (0x75a834 compares the access request, not the group).
    Restricted,
    /// Write access asked of a handle that does not carry it.
    NotWritable,
}

/// A valid handle, decoded. The all-ones word is not a handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Handle {
    pub slot: Slot,
    pub serial: Serial,
    /// Only the handle the slot was opened with may close it.
    pub owner: bool,
    pub writable: bool,
    pub scope: Scope,
    pub tag: Tag,
}

impl Handle {
    /// A handle on the same slot with `scope`, `access` and `tag`, never the owner. `None` keeps
    /// the scope this handle already has.
    pub fn derive(self, scope: Option<AccessGroup>, access: Access, tag: Tag) -> Result<Handle, DeriveRefused> {
        if !self.scope.is_all() && access.bits() != self.scope.word() {
            return Err(DeriveRefused::Restricted);
        }
        if access.writes() && !self.writable {
            return Err(DeriveRefused::NotWritable);
        }
        let scope = scope.map(Scope::one).unwrap_or(self.scope);
        Ok(Handle { owner: false, writable: access.writes(), scope, tag, ..self })
    }
}
