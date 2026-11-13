#![forbid(unsafe_code)]

/// One child job of the measurement round.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lane { Acquisition, Search, Auxiliary, Image, Window, Source, Bookkeeping }

/// The order in which a fan-out signals its lanes.
pub const ORDER: [Lane; 7] = [Lane::Acquisition, Lane::Search, Lane::Auxiliary, Lane::Image, Lane::Window, Lane::Source, Lane::Bookkeeping];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Lanes(u8);

impl Lanes {
    pub const fn empty() -> Self { Self(0) }
    pub const fn all() -> Self { Self(127) }
    pub fn insert(&mut self, lane: Lane) { self.0 |= 1 << lane as u8; }
    pub fn remove(&mut self, lane: Lane) { self.0 &= !(1 << lane as u8); }
    pub const fn contains(self, lane: Lane) -> bool { self.0 & (1 << lane as u8) != 0 }
    pub const fn is_empty(self) -> bool { self.0 == 0 }
    pub const fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
    pub const fn difference(self, other: Self) -> Self { Self(self.0 & !other.0) }
}

impl FromIterator<Lane> for Lanes {
    fn from_iter<I: IntoIterator<Item = Lane>>(lanes: I) -> Self {
        let mut set = Self::empty();
        for lane in lanes { set.insert(lane); }
        set
    }
}

/// Kinds of measurement a client asks for or the record offers. Acquisition
/// covers both the acquisition and window lanes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    pub acquisition: bool,
    pub search: bool,
    pub auxiliary: bool,
    pub image: bool,
    pub source: bool,
}

impl Selection {
    pub const ALL: Self = Self { acquisition: true, search: true, auxiliary: true, image: true, source: true };

    /// Lanes that both selections name. Search and image also need `search_permitted`.
    pub fn shared(self, other: Self, search_permitted: bool) -> Lanes {
        Self {
            acquisition: self.acquisition && other.acquisition,
            search: self.search && other.search && search_permitted,
            auxiliary: self.auxiliary && other.auxiliary,
            image: self.image && other.image && search_permitted,
            source: self.source && other.source,
        }.lanes()
    }

    /// Lanes that `self` names and `other` does not.
    pub fn missing(self, other: Self) -> Lanes {
        Self {
            acquisition: self.acquisition && !other.acquisition,
            search: self.search && !other.search,
            auxiliary: self.auxiliary && !other.auxiliary,
            image: self.image && !other.image,
            source: self.source && !other.source,
        }.lanes()
    }

    fn lanes(self) -> Lanes {
        let mut lanes = Lanes::empty();
        if self.acquisition { lanes.insert(Lane::Acquisition); lanes.insert(Lane::Window); }
        if self.search { lanes.insert(Lane::Search); }
        if self.auxiliary { lanes.insert(Lane::Auxiliary); }
        if self.image { lanes.insert(Lane::Image); }
        if self.source { lanes.insert(Lane::Source); }
        lanes
    }
}
