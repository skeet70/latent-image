//! The task pool and the timer arenas carved from its tail.
#![cfg_attr(not(test), no_std)]

/// A half-open byte range, physical or guest.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Span {
    start: u32,
    end: u32,
}

impl Span {
    #[inline(always)]
    pub const fn new(start: u32, end: u32) -> Option<Self> {
        if start < end {
            Some(Self { start, end })
        } else {
            None
        }
    }

    #[inline(always)]
    pub const fn start(self) -> u32 {
        self.start
    }

    #[inline(always)]
    pub const fn end(self) -> u32 {
        self.end
    }

    #[inline(always)]
    pub const fn len(self) -> u32 {
        self.end - self.start
    }

    #[inline(always)]
    pub const fn contains(self, other: Self) -> bool {
        self.start <= other.start && other.end <= self.end
    }

    #[inline(always)]
    pub const fn overlaps(self, other: Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    #[inline(always)]
    pub const fn aligned(self, align: u32) -> bool {
        align != 0 && align & (align - 1) == 0 && self.start % align == 0 && self.end % align == 0
    }
}

/// The task pool, with the tail the timer arenas own.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskPool {
    pub guest: Span,
    pub allocatable: Span,
    pub reserved: Span,
}

impl TaskPool {
    #[inline(always)]
    pub const fn physical(self) -> Span {
        Span { start: self.guest.start & !0x4000_0000, end: self.guest.end & !0x4000_0000 }
    }

    #[inline(always)]
    pub const fn allocatable_bytes(self) -> u32 {
        self.allocatable.len()
    }

    #[inline(always)]
    pub const fn physical_allocatable(self) -> Span {
        Span { start: self.allocatable.start & !0x4000_0000, end: self.allocatable.end & !0x4000_0000 }
    }
}

/// One timer arena: a node array with a logical capacity inside its owned byte span.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerArena {
    pub guest: Span,
    pub physical: Span,
    pub capacity: u32,
}

impl TimerArena {
    #[inline(always)]
    pub const fn node_bytes(self) -> u32 {
        self.capacity * TIMER_NODE_BYTES
    }

    #[inline(always)]
    pub const fn fits(self) -> bool {
        self.node_bytes() <= self.guest.len()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TimerArenas {
    pub a: TimerArena,
    pub b: TimerArena,
}

/// One timer heap node: expiry and entry pointer.
pub const TIMER_NODE_BYTES: u32 = 0x08;

/// The logical node capacity both domains keep; it is a policy bound, not a recovered stock size.
pub const TIMER_ARENA_CAPACITY: u32 = 184;

const fn arena(start: u32, end: u32, capacity: u32) -> TimerArena {
    TimerArena { guest: Span { start, end }, physical: Span { start: start & !0x4000_0000, end: end & !0x4000_0000 }, capacity }
}

pub const TASK_POOL: TaskPool = TaskPool {
    guest: Span { start: 0x40a1_0200, end: 0x40a5_b200 },
    allocatable: Span { start: 0x40a1_0200, end: 0x40a5_a200 },
    reserved: Span { start: 0x40a5_a200, end: 0x40a5_b200 },
};

pub const TIMER_ARENAS: TimerArenas = TimerArenas {
    a: arena(0x40a5_a200, 0x40a5_aa00, TIMER_ARENA_CAPACITY),
    b: arena(0x40a5_aa00, 0x40a5_b200, TIMER_ARENA_CAPACITY),
};

const _: () = {
    assert!(TASK_POOL.guest.contains(TASK_POOL.allocatable));
    assert!(TASK_POOL.guest.contains(TASK_POOL.reserved));
    assert!(TASK_POOL.allocatable.end() == TASK_POOL.reserved.start());
    assert!(TASK_POOL.allocatable.len() == 0x4a000);
    assert!(TASK_POOL.reserved.len() == 0x1000);
    assert!(TASK_POOL.physical().start() == 0xa10200);
    assert!(TASK_POOL.physical().end() == 0xa5b200);
    assert!(TIMER_ARENAS.a.guest.start() == 0x40a5_a200);
    assert!(TIMER_ARENAS.a.guest.end() == 0x40a5_aa00);
    assert!(TIMER_ARENAS.b.guest.start() == 0x40a5_aa00);
    assert!(TIMER_ARENAS.b.guest.end() == 0x40a5_b200);
    assert!(TIMER_ARENAS.a.physical.start() == 0xa5a200);
    assert!(TIMER_ARENAS.a.physical.end() == 0xa5aa00);
    assert!(TIMER_ARENAS.b.physical.start() == 0xa5aa00);
    assert!(TIMER_ARENAS.b.physical.end() == 0xa5b200);
    assert!(TIMER_ARENAS.a.guest.aligned(32));
    assert!(TIMER_ARENAS.b.guest.aligned(32));
    assert!(!TIMER_ARENAS.a.guest.overlaps(TIMER_ARENAS.b.guest));
    assert!(TASK_POOL.reserved.contains(TIMER_ARENAS.a.guest));
    assert!(TASK_POOL.reserved.contains(TIMER_ARENAS.b.guest));
    assert!(TIMER_ARENAS.a.fits());
    assert!(TIMER_ARENAS.b.fits());
    assert!(TIMER_ARENA_CAPACITY * TIMER_NODE_BYTES == 0x5c0);
};
