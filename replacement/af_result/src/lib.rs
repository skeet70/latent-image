#![no_std]

#[cfg(test)]
extern crate std;

#[cfg(feature = "stage")]
mod abi;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot(pub u32);
impl Slot {
    pub const ANY: Self = Self(0xffff);
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordState(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Primary,
    Secondary,
}
impl Group {
    pub const fn from_raw(value: u32) -> Self {
        if value == 1 { Self::Secondary } else { Self::Primary }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pool {
    pub base: Address,
    pub count: u16,
}
impl Pool {
    fn word(self, slot: Slot, offset: u32) -> Address {
        Address(self.base.0.wrapping_add(slot.0.wrapping_mul(24)).wrapping_add(offset))
    }
    fn state(self, bus: &mut impl Bus, slot: Slot) -> RecordState {
        RecordState(bus.read_word(self.word(slot, 4)))
    }
    fn set_state(self, bus: &mut impl Bus, slot: Slot, state: RecordState) {
        bus.write_word(self.word(slot, 4), state.0);
    }
}

pub trait Bus {
    fn read_word(&mut self, address: Address) -> u32;
    fn write_word(&mut self, address: Address, value: u32);
    fn enter(&mut self) -> u32;
    fn leave(&mut self, token: u32);
}

#[inline(never)]
#[cfg_attr(feature = "stage", link_section = ".text.af_select_core")]
pub fn select(bus: &mut impl Bus, group: Group) -> Pool {
    match group {
        Group::Secondary => Pool { base: Address(0x016fed28), count: 7 },
        Group::Primary => {
            if bus.read_word(Address(0x016fec4c)).wrapping_sub(5) < 2 {
                Pool { base: Address(0x01702f84), count: 4 }
            } else {
                Pool { base: Address(0x016fedd0), count: 5 }
            }
        }
    }
}

fn find_after(bus: &mut impl Bus, pool: Pool, start: u16, state: RecordState) -> Option<Slot> {
    let mut index = start;
    while index < pool.count {
        let slot = Slot(u32::from(index));
        if pool.state(bus, slot) == state {
            return Some(slot);
        }
        index = index.wrapping_add(1);
    }
    None
}

#[cfg_attr(feature = "stage", link_section = ".text.af_rotate_core")]
pub fn rotate(bus: &mut impl Bus, pool: Pool) {
    if pool.count == 0 { return; }
    let first = pool.state(bus, Slot(0));
    let previous = if first == RecordState(3) {
        Some(Slot(0))
    } else {
        find_after(bus, pool, 1, RecordState(3))
    };
    let active = if first == RecordState(2) {
        Some(Slot(0))
    } else {
        find_after(bus, pool, 1, RecordState(2))
    };
    if let Some(slot) = active {
        let policy = bus.read_word(Address(0x0170319c));
        pool.set_state(bus, slot, RecordState(if policy == 1 { 3 } else { 0 }));
    }
    if let Some(slot) = previous {
        let word = bus.read_word(pool.word(slot, 0x10));
        pool.set_state(bus, slot, RecordState(if word == 0 { 0 } else { 0xd }));
    }
    let current = bus.read_word(Address(0x01703080));
    let completed = bus.read_word(Address(0x01703084));
    let wanted = RecordState(if completed < current { 0x1b } else { 1 });
    if let Some(slot) = find_after(bus, pool, 0, wanted) {
        pool.set_state(bus, slot, RecordState(2));
    }
}

pub fn reserve(bus: &mut impl Bus, requested: Slot, group: Group) -> Slot {
    let token = bus.enter();
    let pool = select(bus, group);
    let result = reserve_in_pool(bus, pool, requested);
    bus.leave(token);
    result
}

fn reserve_in_pool(bus: &mut impl Bus, pool: Pool, requested: Slot) -> Slot {
    if requested != Slot::ANY {
        return mark_free(bus, pool, requested);
    }
    let mut index = 0u16;
    while index < pool.count {
        let completed = bus.read_word(Address(0x01703084)).wrapping_add(1);
        let current = bus.read_word(Address(0x01703080));
        let slot = Slot(u32::from(index));
        if completed < current {
            if let Some(previous) = find_after(bus, pool, 0, RecordState(3)) {
                pool.set_state(bus, previous, RecordState(0));
            }
            if pool.state(bus, slot) == RecordState(1) {
                pool.set_state(bus, slot, RecordState(0x1b));
                break;
            }
        } else if pool.state(bus, slot) == RecordState(1) {
            return slot;
        }
        index = index.wrapping_add(1);
    }
    match find_after(bus, pool, 0, RecordState(0)) {
        Some(slot) => mark_selected_free(bus, pool, slot),
        None => Slot::ANY,
    }
}

fn mark_free(bus: &mut impl Bus, pool: Pool, slot: Slot) -> Slot {
    if pool.state(bus, slot) != RecordState(0) {
        return Slot::ANY;
    }
    mark_selected_free(bus, pool, slot)
}

fn mark_selected_free(bus: &mut impl Bus, pool: Pool, slot: Slot) -> Slot {
    let mark = bus.read_word(Address(0x0170306c));
    pool.set_state(bus, slot, RecordState(if mark == 1 { 2 } else { 1 }));
    slot
}

pub fn release(bus: &mut impl Bus, group: Group) {
    let token = bus.enter();
    let pool = select(bus, group);
    rotate(bus, pool);
    bus.leave(token);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SecondaryBank {
    pub base: Address,
}
impl SecondaryBank {
    pub const COUNT: u16 = 7;
    pub const fn new(base: Address) -> Self {
        Self { base }
    }
    #[inline(always)]
    fn record(self, slot: Slot) -> Record {
        Record(Address(self.base.0.wrapping_add(slot.0.wrapping_mul(24))))
    }
    #[inline(always)]
    fn state(self, bus: &mut impl Bus, slot: Slot) -> RecordState {
        self.record(slot).state(bus)
    }
    #[inline(always)]
    fn set_state(self, bus: &mut impl Bus, slot: Slot, state: RecordState) {
        self.record(slot).set_state(bus, state);
    }
    #[inline(always)]
    fn flags(self, bus: &mut impl Bus, slot: Slot) -> u32 {
        self.record(slot).flags(bus)
    }
    #[inline(always)]
    fn set_flags(self, bus: &mut impl Bus, slot: Slot, flags: u32) {
        self.record(slot).set_flags(bus, flags);
    }
    #[inline(always)]
    fn find(self, bus: &mut impl Bus, state: RecordState) -> Option<Slot> {
        let mut index = 0u16;
        while index < Self::COUNT {
            let slot = Slot(u32::from(index));
            if self.state(bus, slot) == state {
                return Some(slot);
            }
            index = index.wrapping_add(1);
        }
        None
    }
    #[inline(always)]
    fn find_flag(self, bus: &mut impl Bus, flag: u32) -> Option<Slot> {
        let mut index = 0u16;
        while index < Self::COUNT {
            let slot = Slot(u32::from(index));
            if self.flags(bus, slot) & flag != 0 {
                return Some(slot);
            }
            index = index.wrapping_add(1);
        }
        None
    }
}

#[derive(Clone, Copy)]
struct Record(Address);
impl Record {
    #[inline(always)]
    fn state(self, bus: &mut impl Bus) -> RecordState {
        RecordState(bus.read_word(Address(self.0.0.wrapping_add(4))))
    }
    #[inline(always)]
    fn set_state(self, bus: &mut impl Bus, state: RecordState) {
        bus.write_word(Address(self.0.0.wrapping_add(4)), state.0);
    }
    #[inline(always)]
    fn flags(self, bus: &mut impl Bus) -> u32 {
        bus.read_word(Address(self.0.0.wrapping_add(16)))
    }
    #[inline(always)]
    fn set_flags(self, bus: &mut impl Bus, flags: u32) {
        bus.write_word(Address(self.0.0.wrapping_add(16)), flags);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumerMode {
    Normal,
    Tagged,
}
impl ConsumerMode {
    pub const fn from_raw(value: u32) -> Self {
        if value == 1 { Self::Tagged } else { Self::Normal }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    FirstMarked,
    FirstCurrent,
}
impl Route {
    pub const fn from_raw(value: u32) -> Self {
        if value == 1 { Self::FirstMarked } else { Self::FirstCurrent }
    }
}

#[inline(never)]
#[cfg_attr(feature = "stage", link_section = ".af_secondary_core")]
pub fn consume(bus: &mut impl Bus, bank: SecondaryBank, mode: ConsumerMode, route: Route) {
    match mode {
        ConsumerMode::Normal => consume_normal(bus, bank, route),
        ConsumerMode::Tagged => consume_tagged(bus, bank),
    }
}

#[inline(always)]
fn consume_normal(bus: &mut impl Bus, bank: SecondaryBank, route: Route) {
    if let Some(slot) = bank.find(bus, RecordState(6)) {
        bank.set_state(bus, slot, RecordState(0));
    }
    let prior = bank.find(bus, RecordState(5));
    let candidate_state = match route {
        Route::FirstMarked => RecordState(2),
        Route::FirstCurrent => RecordState(3),
    };
    if let Some(slot) = bank.find(bus, candidate_state) {
        bank.set_state(bus, slot, RecordState(5));
        if let Some(prior) = prior {
            bank.set_state(bus, prior, RecordState(6));
        }
    }
}

#[inline(always)]
fn consume_tagged(bus: &mut impl Bus, bank: SecondaryBank) {
    if let Some(slot) = bank.find(bus, RecordState(6)) {
        bank.set_state(bus, slot, RecordState(0));
        let flags = bank.flags(bus, slot);
        bank.set_flags(bus, slot, flags & !0x10);
    }
    if let Some(selected) = bank.find_flag(bus, 0x40000) {
        let mut index = 0u16;
        while index < SecondaryBank::COUNT {
            let slot = Slot(u32::from(index));
            if bank.state(bus, slot) == RecordState(5) {
                bank.set_state(bus, slot, RecordState(6));
            }
            index = index.wrapping_add(1);
        }
        let flags = bank.flags(bus, selected);
        bank.set_flags(bus, selected, flags & !0x40000);
        bank.set_state(bus, selected, RecordState(5));
    }
}
