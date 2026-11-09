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
