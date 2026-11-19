//! Request tags handed to stock callers and the map from tags to file indexes.

use core::num::NonZeroU32;

/// One of the 16 tag slots or the 16 map pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot(u8);

impl Slot {
    pub const COUNT: usize = 16;

    pub fn all() -> impl Iterator<Item = Slot> {
        (0..Self::COUNT as u8).map(Slot)
    }

    pub fn index(self) -> usize {
        usize::from(self.0)
    }
}

/// A tag held by a stock caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tag(NonZeroU32);

impl Tag {
    pub fn new(word: u32) -> Option<Tag> {
        NonZeroU32::new(word).map(Tag)
    }

    pub fn word(self) -> u32 {
        self.0.get()
    }
}

/// A file index from the file cache, 1-based; zero is no index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileIndex(pub u32);

/// One map pair: a file index bound to the tag word. A zero tag word marks a free pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pair {
    pub index: FileIndex,
    pub tag: u32,
}

impl Pair {
    pub const FREE: Pair = Pair { index: FileIndex(0), tag: 0 };
}

pub trait HandleMemory {
    fn tag(&self, slot: Slot) -> u32;
    fn set_tag(&mut self, slot: Slot, word: u32);
    fn sequence(&self) -> u32;
    fn set_sequence(&mut self, word: u32);
    fn pair(&self, slot: Slot) -> Pair;
    fn set_pair(&mut self, slot: Slot, pair: Pair);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acquired {
    Yes,
    No,
}

/// Semaphore 0x3c, which serializes the tag slots.
pub trait TagLock {
    fn acquire(&mut self) -> Acquired;
    fn release(&mut self);
}

pub trait FileCache {
    fn claim(&mut self) -> FileIndex;
}

#[cfg(not(file_service_control_sequence_wrap))]
fn next_candidate(previous: u32) -> u32 {
    let next = previous.wrapping_add(1);
    if (next as i32) < 0 {
        1
    } else {
        next
    }
}

#[cfg(file_service_control_sequence_wrap)]
fn next_candidate(previous: u32) -> u32 {
    previous.wrapping_add(1)
}

fn holds<M: HandleMemory>(memory: &M, word: u32) -> bool {
    Slot::all().any(|slot| memory.tag(slot) == word)
}

#[cfg(not(file_service_control_skip_allocated))]
fn candidate_taken<M: HandleMemory>(memory: &M, word: u32) -> bool {
    holds(memory, word)
}

#[cfg(file_service_control_skip_allocated)]
fn candidate_taken<M: HandleMemory>(_: &M, _: u32) -> bool {
    false
}

/// Advances the sequence past every word a slot holds and stores the new tag in the
/// first free slot. With no free slot the sequence still advances and no tag is
/// returned. A failed wait returns no tag and leaves the lock alone.
pub fn allocate<M: HandleMemory, L: TagLock>(memory: &mut M, lock: &mut L) -> Option<Tag> {
    if lock.acquire() == Acquired::No {
        return None;
    }
    let mut candidate = memory.sequence();
    loop {
        candidate = next_candidate(candidate);
        if !candidate_taken(memory, candidate) {
            break;
        }
    }
    let free = Slot::all().find(|&slot| memory.tag(slot) == 0);
    #[cfg(not(file_service_control_no_free_slot_advance))]
    memory.set_sequence(candidate);
    #[cfg(file_service_control_no_free_slot_advance)]
    if free.is_some() {
        memory.set_sequence(candidate);
    }
    let allocated = free.and_then(|slot| {
        memory.set_tag(slot, candidate);
        Tag::new(candidate)
    });
    lock.release();
    allocated
}

/// Clears the first slot holding `tag`, releasing the lock even when its wait failed.
pub fn release<M: HandleMemory, L: TagLock>(memory: &mut M, lock: &mut L, tag: Option<Tag>) {
    let Some(tag) = tag else {
        return;
    };
    let acquired = lock.acquire();
    if acquired == Acquired::Yes {
        if let Some(slot) = Slot::all().find(|&slot| memory.tag(slot) == tag.word()) {
            memory.set_tag(slot, 0);
        }
    }
    #[cfg(file_service_control_release_after_failed_wait)]
    if acquired == Acquired::No {
        return;
    }
    lock.release();
}

/// Claims a file index and binds it to `tag` in the first free pair. With every pair in
/// use the index is still returned, unbound.
pub fn bind<M: HandleMemory, C: FileCache>(memory: &mut M, cache: &mut C, tag: u32) -> FileIndex {
    let index = cache.claim();
    if index.0 == 0 {
        return index;
    }
    if let Some(slot) = Slot::all().find(|&slot| memory.pair(slot).tag == 0) {
        memory.set_pair(slot, Pair { index, tag });
    }
    index
}

/// The file index the first pair bound to `tag` holds.
pub fn lookup<M: HandleMemory>(memory: &M, tag: Option<Tag>) -> FileIndex {
    tag.and_then(|tag| Slot::all().map(|slot| memory.pair(slot)).find(|pair| pair.tag == tag.word()))
        .map_or(FileIndex(0), |pair| pair.index)
}

/// Frees the first pair bound to `tag`.
pub fn unbind<M: HandleMemory>(memory: &mut M, tag: Option<Tag>) {
    let Some(tag) = tag else {
        return;
    };
    if let Some(slot) = Slot::all().find(|&slot| memory.pair(slot).tag == tag.word()) {
        memory.set_pair(slot, Pair::FREE);
    }
}
