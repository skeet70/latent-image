//! Allocation-free TIMER host core: the stock 1-based min-heap and the zero-token
//! membership protocol. `KernelContext` fields hold pointers to the node array, count,
//! base and tick; the capacity is a validated caller parameter.
#![cfg_attr(not(test), no_std)]

#[path = "../../rtos_layout/src/lib.rs"]
pub mod layout;

use core::num::NonZeroU32;
use layout::{GuestAddress, KernelContext};

pub trait Machine {
    fn load_word(&self, at: GuestAddress) -> u32;
    fn store_word(&self, at: GuestAddress, value: u32);
}

pub trait Scheduler: Machine {
    /// Zero in task context.
    fn handler_context(&self) -> u32;
    fn irq_masked(&self) -> bool;
    fn release_lock(&self);
    fn acquire_lock(&self);
    fn irq_clear(&self);
    fn irq_set(&self);
}

const NODE_SIZE: u32 = 0x08;
const NODE_EXPIRY: u32 = 0x00;
const NODE_ENTRY: u32 = 0x04;

/// The largest capacity whose `(index - 1) * 8` node offset and doubled child index stay in u32.
const MAX_CAPACITY: u32 = (u32::MAX / NODE_SIZE) + 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Capacity(NonZeroU32);

impl Capacity {
    pub fn new(value: u32) -> Option<Self> {
        if value == 0 || value > MAX_CAPACITY {
            return None;
        }
        NonZeroU32::new(value).map(Self)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Index(NonZeroU32);

impl Index {
    fn new(value: u32, capacity: Capacity) -> Result<Self, TimerError> {
        if value == 0 || value > capacity.get() {
            return Err(TimerError::CorruptIndex);
        }
        NonZeroU32::new(value).map(Self).ok_or(TimerError::CorruptIndex)
    }

    pub fn get(self) -> u32 {
        self.0.get()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerIndex {
    Detached,
    Linked(Index),
}

/// A view identity of the 12-byte entry record: index +0x00, callback +0x04, argument +0x08.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerEntry(GuestAddress);

impl TimerEntry {
    pub fn at(address: GuestAddress) -> Self {
        Self(address)
    }

    pub fn address(self) -> GuestAddress {
        self.0
    }

    pub fn index<M: Machine>(self, m: &M, capacity: Capacity) -> Result<TimerIndex, TimerError> {
        let word = m.load_word(self.0);
        if word == 0 {
            Ok(TimerIndex::Detached)
        } else {
            Index::new(word, capacity).map(TimerIndex::Linked)
        }
    }

    pub fn callback<M: Machine>(self, m: &M) -> CallbackAddress {
        CallbackAddress(m.load_word(self.0.offset(0x04)))
    }

    pub fn argument<M: Machine>(self, m: &M) -> CallbackArgument {
        CallbackArgument(m.load_word(self.0.offset(0x08)))
    }

    fn set_index<M: Machine>(self, m: &M, index: TimerIndex) {
        let word = match index {
            TimerIndex::Detached => 0,
            TimerIndex::Linked(index) => index.get(),
        };
        m.store_word(self.0, word);
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackAddress(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CallbackArgument(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Expiry(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Now(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Base(pub u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerError {
    Context,
    InterruptMasked,
    CorruptIndex,
    CorruptCount,
    AlreadyLinked,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Removed {
    Noop,
    Detached,
}

#[derive(Clone, Copy, Debug)]
pub struct TimerHeap {
    nodes: GuestAddress,
    count: GuestAddress,
    base: GuestAddress,
    tick: GuestAddress,
    capacity: Capacity,
}

impl TimerHeap {
    pub fn from_context<M: Machine>(m: &M, context: KernelContext, capacity: Capacity) -> Self {
        Self {
            nodes: GuestAddress(m.load_word(context.timer_heap())),
            count: GuestAddress(m.load_word(context.timer_count())),
            base: GuestAddress(m.load_word(context.timer_base())),
            tick: GuestAddress(m.load_word(context.tick_now())),
            capacity,
        }
    }

    pub fn capacity(self) -> Capacity {
        self.capacity
    }

    fn count<M: Machine>(self, m: &M) -> u32 {
        m.load_word(self.count)
    }

    fn set_count<M: Machine>(self, m: &M, value: u32) {
        m.store_word(self.count, value);
    }

    fn base<M: Machine>(self, m: &M) -> Base {
        Base(m.load_word(self.base))
    }

    fn tick_now<M: Machine>(self, m: &M) -> Now {
        Now(m.load_word(self.tick))
    }

    fn set_base<M: Machine>(self, m: &M, value: Base) {
        m.store_word(self.base, value.0);
    }

    fn set_tick_now<M: Machine>(self, m: &M, value: Now) {
        m.store_word(self.tick, value.0);
    }

    fn node(self, index: Index) -> GuestAddress {
        self.nodes.offset((index.get() - 1) * NODE_SIZE)
    }
}

fn node_expiry<M: Machine>(m: &M, heap: TimerHeap, index: Index) -> Expiry {
    Expiry(m.load_word(heap.node(index)))
}

fn node_entry<M: Machine>(m: &M, heap: TimerHeap, index: Index) -> TimerEntry {
    TimerEntry(GuestAddress(m.load_word(heap.node(index).offset(NODE_ENTRY))))
}

fn relative(value: u32, base: Base) -> u32 {
    value.wrapping_sub(base.0)
}

fn sift_up<M: Machine>(m: &M, heap: TimerHeap, mut index: Index, expiry: Expiry, base: Base) -> Index {
    while index.get() > 1 {
        let parent = Index::new(index.get() >> 1, heap.capacity).expect("parent slot is in range");
        let parent_expiry = node_expiry(m, heap, parent);
        if relative(parent_expiry.0, base) <= relative(expiry.0, base) {
            break;
        }
        let parent_entry = node_entry(m, heap, parent);
        let node = heap.node(index);
        m.store_word(node.offset(NODE_EXPIRY), parent_expiry.0);
        m.store_word(node.offset(NODE_ENTRY), parent_entry.address().0);
        parent_entry.set_index(m, TimerIndex::Linked(index));
        index = parent;
    }
    index
}

fn sift_down<M: Machine>(m: &M, heap: TimerHeap, mut index: Index, expiry: Expiry, base: Base, count: u32) -> Index {
    loop {
        let child = index.get() * 2;
        if child > count {
            break;
        }
        let left = Index::new(child, heap.capacity).expect("child slot is in range");
        let left_expiry = node_expiry(m, heap, left);
        let (smaller, smaller_expiry) = match Index::new(child + 1, heap.capacity) {
            Ok(right) if child + 1 <= count => {
                let right_expiry = node_expiry(m, heap, right);
                if relative(right_expiry.0, base) < relative(left_expiry.0, base) {
                    (right, right_expiry)
                } else {
                    (left, left_expiry)
                }
            }
            _ => (left, left_expiry),
        };
        if relative(expiry.0, base) <= relative(smaller_expiry.0, base) {
            break;
        }
        let smaller_entry = node_entry(m, heap, smaller);
        let node = heap.node(index);
        m.store_word(node.offset(NODE_EXPIRY), smaller_expiry.0);
        m.store_word(node.offset(NODE_ENTRY), smaller_entry.address().0);
        smaller_entry.set_index(m, TimerIndex::Linked(index));
        index = smaller;
    }
    index
}

/// Refuses an already-linked entry without a write.
pub fn insert<M: Machine>(m: &M, heap: TimerHeap, entry: TimerEntry, expiry: Expiry) -> Result<Index, TimerError> {
    if let TimerIndex::Linked(_) = entry.index(m, heap.capacity)? {
        return Err(TimerError::AlreadyLinked);
    }
    let count = heap.count(m);
    if count >= heap.capacity.get() {
        return Err(TimerError::CorruptCount);
    }
    let count = count + 1;
    let index = Index::new(count, heap.capacity)?;
    heap.set_count(m, count);
    let base = heap.base(m);
    let index = sift_up(m, heap, index, expiry, base);
    let node = heap.node(index);
    m.store_word(node.offset(NODE_EXPIRY), expiry.0);
    m.store_word(node.offset(NODE_ENTRY), entry.address().0);
    entry.set_index(m, TimerIndex::Linked(index));
    Ok(index)
}

/// A detached entry returns `Noop` before any heap read, write or count change.
pub fn remove<M: Machine>(m: &M, heap: TimerHeap, entry: TimerEntry) -> Result<Removed, TimerError> {
    let index = match entry.index(m, heap.capacity)? {
        TimerIndex::Detached => return Ok(Removed::Noop),
        TimerIndex::Linked(index) => index,
    };
    let count = heap.count(m);
    if count > heap.capacity.get() || count == 0 || index.get() > count {
        return Err(TimerError::CorruptCount);
    }
    if node_entry(m, heap, index).address().0 != entry.address().0 {
        return Err(TimerError::CorruptIndex);
    }
    entry.set_index(m, TimerIndex::Detached);
    let new_count = count - 1;
    heap.set_count(m, new_count);
    if new_count == 0 || index.get() == count {
        return Ok(Removed::Detached);
    }
    let last = Index::new(count, heap.capacity)?;
    let last_expiry = node_expiry(m, heap, last);
    let last_entry = node_entry(m, heap, last);
    let base = heap.base(m);
    let index = if index.get() > 1 {
        let parent = Index::new(index.get() >> 1, heap.capacity)?;
        if relative(last_expiry.0, base) < relative(node_expiry(m, heap, parent).0, base) {
            sift_up(m, heap, index, last_expiry, base)
        } else {
            sift_down(m, heap, index, last_expiry, base, new_count)
        }
    } else {
        sift_down(m, heap, index, last_expiry, base, new_count)
    };
    let node = heap.node(index);
    m.store_word(node.offset(NODE_EXPIRY), last_expiry.0);
    m.store_word(node.offset(NODE_ENTRY), last_entry.address().0);
    last_entry.set_index(m, TimerIndex::Linked(index));
    Ok(Removed::Detached)
}

/// The callback receives the lock held and must return it held; it may release and reacquire
/// meanwhile.
pub fn tick<M: Scheduler>(
    m: &M,
    heap: TimerHeap,
    mut invoke: impl FnMut(&M, CallbackAddress, CallbackArgument),
) -> Result<(), TimerError> {
    if m.handler_context() == 0 {
        return Err(TimerError::Context);
    }
    if m.irq_masked() {
        return Err(TimerError::InterruptMasked);
    }
    m.irq_set();
    m.acquire_lock();
    loop {
        let count = heap.count(m);
        if count == 0 {
            break;
        }
        if count > heap.capacity.get() {
            m.release_lock();
            m.irq_clear();
            return Err(TimerError::CorruptCount);
        }
        let base = heap.base(m);
        let tick = heap.tick_now(m);
        let head = match Index::new(1, heap.capacity) {
            Ok(head) => head,
            Err(error) => {
                m.release_lock();
                m.irq_clear();
                return Err(error);
            }
        };
        if relative(node_expiry(m, heap, head).0, base) > relative(tick.0, base) {
            break;
        }
        let entry = node_entry(m, heap, head);
        let callback = entry.callback(m);
        let argument = entry.argument(m);
        entry.set_index(m, TimerIndex::Detached);
        let new_count = count - 1;
        heap.set_count(m, new_count);
        if new_count != 0 {
            let last = match Index::new(count, heap.capacity) {
                Ok(last) => last,
                Err(error) => {
                    m.release_lock();
                    m.irq_clear();
                    return Err(error);
                }
            };
            let last_expiry = node_expiry(m, heap, last);
            let last_entry = node_entry(m, heap, last);
            let index = sift_down(m, heap, head, last_expiry, base, new_count);
            let node = heap.node(index);
            m.store_word(node.offset(NODE_EXPIRY), last_expiry.0);
            m.store_word(node.offset(NODE_ENTRY), last_entry.address().0);
            last_entry.set_index(m, TimerIndex::Linked(index));
        }
        invoke(m, callback, argument);
        m.release_lock();
        m.irq_clear();
        m.irq_set();
        m.acquire_lock();
    }
    let tick = heap.tick_now(m);
    heap.set_base(m, Base(tick.0));
    heap.set_tick_now(m, Now(tick.0.wrapping_add(10)));
    m.release_lock();
    m.irq_clear();
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(all(target_arch = "arm", not(test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {}
}
