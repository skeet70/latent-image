use crate::handles::*;

#[derive(Default)]
struct Memory {
    tags: [u32; 16],
    sequence: u32,
    pairs: [(u32, u32); 16],
}

impl HandleMemory for Memory {
    fn tag(&self, slot: Slot) -> u32 {
        self.tags[slot.index()]
    }
    fn set_tag(&mut self, slot: Slot, word: u32) {
        self.tags[slot.index()] = word;
    }
    fn sequence(&self) -> u32 {
        self.sequence
    }
    fn set_sequence(&mut self, word: u32) {
        self.sequence = word;
    }
    fn pair(&self, slot: Slot) -> Pair {
        let (index, tag) = self.pairs[slot.index()];
        Pair { index: FileIndex(index), tag }
    }
    fn set_pair(&mut self, slot: Slot, pair: Pair) {
        self.pairs[slot.index()] = (pair.index.0, pair.tag);
    }
}

#[derive(Debug, PartialEq)]
enum LockEvent {
    Acquire,
    Release,
}

struct Lock {
    grants: bool,
    events: Vec<LockEvent>,
}

impl Lock {
    fn granting() -> Lock {
        Lock { grants: true, events: Vec::new() }
    }
    fn refusing() -> Lock {
        Lock { grants: false, events: Vec::new() }
    }
}

impl TagLock for Lock {
    fn acquire(&mut self) -> Acquired {
        self.events.push(LockEvent::Acquire);
        if self.grants {
            Acquired::Yes
        } else {
            Acquired::No
        }
    }
    fn release(&mut self) {
        self.events.push(LockEvent::Release);
    }
}

struct Cache(u32);

impl FileCache for Cache {
    fn claim(&mut self) -> FileIndex {
        FileIndex(self.0)
    }
}

fn word(tag: Option<Tag>) -> u32 {
    tag.map_or(0, Tag::word)
}

#[test]
fn allocation_takes_the_next_sequence_word_into_the_first_free_slot() {
    let mut memory = Memory { sequence: 41, ..Memory::default() };
    memory.tags[0] = 7;
    let mut lock = Lock::granting();
    assert_eq!(word(allocate(&mut memory, &mut lock)), 42);
    assert_eq!(memory.tags[..3], [7, 42, 0]);
    assert_eq!(memory.sequence, 42);
    assert_eq!(lock.events, [LockEvent::Acquire, LockEvent::Release]);
}

#[test]
fn allocation_wraps_from_the_largest_positive_word_to_one() {
    let mut memory = Memory { sequence: 0x7fff_ffff, ..Memory::default() };
    assert_eq!(word(allocate(&mut memory, &mut Lock::granting())), 1);
    assert_eq!(memory.sequence, 1);
}

#[test]
fn a_zero_candidate_is_skipped_while_a_slot_is_free() {
    let mut memory = Memory { sequence: 0xffff_ffff, ..Memory::default() };
    assert_eq!(word(allocate(&mut memory, &mut Lock::granting())), 1);
}

#[test]
fn allocation_skips_words_already_held() {
    let mut memory = Memory { sequence: 9, ..Memory::default() };
    memory.tags[3] = 10;
    memory.tags[15] = 11;
    assert_eq!(word(allocate(&mut memory, &mut Lock::granting())), 12);
    assert_eq!(memory.tags[0], 12);
}

#[test]
fn a_full_table_advances_the_sequence_and_returns_no_tag() {
    let mut memory = Memory { sequence: 100, ..Memory::default() };
    for (slot, tag) in memory.tags.iter_mut().zip(200..) {
        *slot = tag;
    }
    let mut lock = Lock::granting();
    assert_eq!(allocate(&mut memory, &mut lock), None);
    assert_eq!(memory.sequence, 101);
    assert_eq!(lock.events, [LockEvent::Acquire, LockEvent::Release]);
}

#[test]
fn a_failed_wait_allocates_nothing_and_releases_nothing() {
    let mut memory = Memory { sequence: 5, ..Memory::default() };
    let mut lock = Lock::refusing();
    assert_eq!(allocate(&mut memory, &mut lock), None);
    assert_eq!(memory.sequence, 5);
    assert_eq!(memory.tags, [0; 16]);
    assert_eq!(lock.events, [LockEvent::Acquire]);
}

#[test]
fn release_clears_the_first_slot_holding_the_tag() {
    let mut memory = Memory::default();
    memory.tags[2] = 9;
    memory.tags[4] = 9;
    let mut lock = Lock::granting();
    release(&mut memory, &mut lock, Tag::new(9));
    assert_eq!(memory.tags[2], 0);
    assert_eq!(memory.tags[4], 9);
    assert_eq!(lock.events, [LockEvent::Acquire, LockEvent::Release]);
}

#[test]
fn release_of_no_tag_takes_no_lock() {
    let mut lock = Lock::granting();
    release(&mut Memory::default(), &mut lock, None);
    assert!(lock.events.is_empty());
}

#[test]
fn release_after_a_failed_wait_clears_nothing_but_releases() {
    let mut memory = Memory::default();
    memory.tags[0] = 3;
    let mut lock = Lock::refusing();
    release(&mut memory, &mut lock, Tag::new(3));
    assert_eq!(memory.tags[0], 3);
    assert_eq!(lock.events, [LockEvent::Acquire, LockEvent::Release]);
}

#[test]
fn bind_records_the_claimed_index_in_the_first_free_pair() {
    let mut memory = Memory::default();
    memory.pairs[0] = (1, 77);
    assert_eq!(bind(&mut memory, &mut Cache(2), 78), FileIndex(2));
    assert_eq!(memory.pairs[1], (2, 78));
    assert_eq!(lookup(&memory, Tag::new(78)), FileIndex(2));
    assert_eq!(lookup(&memory, Tag::new(77)), FileIndex(1));
}

#[test]
fn bind_without_a_claim_records_nothing() {
    let mut memory = Memory::default();
    assert_eq!(bind(&mut memory, &mut Cache(0), 5), FileIndex(0));
    assert_eq!(memory.pairs, [(0, 0); 16]);
}

#[test]
fn bind_with_every_pair_in_use_returns_the_index_unbound() {
    let mut memory = Memory::default();
    for (pair, tag) in memory.pairs.iter_mut().zip(1..) {
        *pair = (tag, tag);
    }
    assert_eq!(bind(&mut memory, &mut Cache(3), 99), FileIndex(3));
    assert_eq!(lookup(&memory, Tag::new(99)), FileIndex(0));
}

#[test]
fn unbind_frees_the_first_pair_for_the_tag() {
    let mut memory = Memory::default();
    memory.pairs[5] = (4, 21);
    memory.pairs[6] = (6, 21);
    unbind(&mut memory, Tag::new(21));
    assert_eq!(memory.pairs[5], (0, 0));
    assert_eq!(lookup(&memory, Tag::new(21)), FileIndex(6));
}

#[test]
fn no_tag_looks_up_nothing_and_unbinds_nothing() {
    let mut memory = Memory::default();
    memory.pairs[0] = (8, 0);
    assert_eq!(lookup(&memory, None), FileIndex(0));
    unbind(&mut memory, None);
    assert_eq!(memory.pairs[0], (8, 0));
}
