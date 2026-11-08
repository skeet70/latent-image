//! Host model of the local exclusive monitor and the three services' observable effects.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::*;

const GLOBAL: u32 = 0x400c_0804;
const EXPLICIT: u32 = 0x409c_2808;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Event {
    Load(u32),
    LoadExclusive(u32),
    StoreExclusive { at: u32, value: u32, status: u32 },
    Barrier,
}

#[derive(Default)]
struct State {
    words: BTreeMap<u32, u32>,
    reserved: Option<u32>,
    lose_next_store: bool,
    holder_on_loss: Option<u32>,
    events: Vec<Event>,
}

struct Sim(RefCell<State>);

impl Sim {
    fn new() -> Self {
        Self(RefCell::new(State::default()))
    }

    fn with_word(self, at: u32, value: u32) -> Self {
        self.0.borrow_mut().words.insert(at, value);
        self
    }

    fn lose_next_store(&self) {
        self.0.borrow_mut().lose_next_store = true;
    }

    fn lose_next_store_to(&self, holder: u32) {
        let mut state = self.0.borrow_mut();
        state.lose_next_store = true;
        state.holder_on_loss = Some(holder);
    }

    fn word(&self, at: u32) -> u32 {
        self.0.borrow().words.get(&at).copied().unwrap_or(0)
    }

    fn events(&self) -> Vec<Event> {
        self.0.borrow().events.clone()
    }
}

impl Machine for Sim {
    fn load_word(&self, at: GuestAddress) -> u32 {
        let mut state = self.0.borrow_mut();
        state.events.push(Event::Load(at.0));
        state.words.get(&at.0).copied().unwrap_or(0)
    }

    fn exclusive_load(&self, at: GuestAddress) -> u32 {
        let mut state = self.0.borrow_mut();
        state.events.push(Event::LoadExclusive(at.0));
        state.reserved = Some(at.0);
        state.words.get(&at.0).copied().unwrap_or(0)
    }

    fn exclusive_store(&self, at: GuestAddress, value: u32) -> u32 {
        let mut state = self.0.borrow_mut();
        let committed = state.reserved == Some(at.0) && !state.lose_next_store;
        let status = if committed { 0 } else { 1 };
        if committed {
            state.words.insert(at.0, value);
        } else {
            state.lose_next_store = false;
            if let Some(holder) = state.holder_on_loss.take() {
                state.words.insert(at.0, holder);
            }
        }
        state.reserved = None;
        state.events.push(Event::StoreExclusive { at: at.0, value, status });
        status
    }

    fn data_memory_barrier(&self) {
        self.0.borrow_mut().events.push(Event::Barrier);
    }
}

fn exclusive(at: u32) -> Event {
    Event::LoadExclusive(at)
}

fn store(at: u32, value: u32, status: u32) -> Event {
    Event::StoreExclusive { at, value, status }
}

const BARRIER: Event = Event::Barrier;

#[test]
fn a_zero_argument_selects_the_global_word() {
    assert_eq!(LockWord::from_argument(0), LockWord::DEFAULT_GLOBAL);
    assert_eq!(LockWord::from_argument(0).address().0, GLOBAL);
}

#[test]
fn a_nonzero_argument_is_the_caller_word() {
    assert_eq!(LockWord::from_argument(EXPLICIT).address().0, EXPLICIT);
}

#[test]
fn try_acquires_a_free_word_and_barriers_after_the_store() {
    let sim = Sim::new();
    let result = try_acquire(&sim, LockWord::DEFAULT_GLOBAL, LockStamp(0x11));
    assert_eq!(result, TryResult::Acquired);
    assert_eq!(sim.word(GLOBAL), 0x11);
    assert_eq!(sim.events(), vec![exclusive(GLOBAL), store(GLOBAL, 0x11, 0), BARRIER]);
}

#[test]
fn try_refuses_a_held_word_without_a_store_or_barrier() {
    let sim = Sim::new().with_word(GLOBAL, 0x12);
    let result = try_acquire(&sim, LockWord::DEFAULT_GLOBAL, LockStamp(0x11));
    assert_eq!(result, TryResult::Held);
    assert_eq!(sim.word(GLOBAL), 0x12);
    assert_eq!(sim.events(), vec![exclusive(GLOBAL)]);
}

#[test]
fn try_stores_a_zero_stamp_unchecked() {
    let sim = Sim::new();
    assert_eq!(try_acquire(&sim, LockWord::DEFAULT_GLOBAL, LockStamp(0)), TryResult::Acquired);
    assert_eq!(sim.word(GLOBAL), 0);
}

#[test]
fn try_stores_an_arbitrary_stamp_unchecked() {
    let sim = Sim::new();
    assert_eq!(try_acquire(&sim, LockWord::DEFAULT_GLOBAL, LockStamp(0xdead_beef)), TryResult::Acquired);
    assert_eq!(sim.word(GLOBAL), 0xdead_beef);
}

#[test]
fn try_repeats_the_exclusive_pair_after_a_lost_store() {
    let sim = Sim::new();
    sim.lose_next_store();
    assert_eq!(try_acquire(&sim, LockWord::DEFAULT_GLOBAL, LockStamp(0x21)), TryResult::Acquired);
    assert_eq!(sim.word(GLOBAL), 0x21);
    assert_eq!(
        sim.events(),
        vec![exclusive(GLOBAL), store(GLOBAL, 0x21, 1), exclusive(GLOBAL), store(GLOBAL, 0x21, 0), BARRIER]
    );
}

#[test]
fn try_refuses_when_a_holder_wins_the_retry() {
    let sim = Sim::new();
    sim.lose_next_store_to(0x22);
    assert_eq!(try_acquire(&sim, LockWord::DEFAULT_GLOBAL, LockStamp(0x21)), TryResult::Held);
    assert_eq!(sim.word(GLOBAL), 0x22);
    assert_eq!(sim.events(), vec![exclusive(GLOBAL), store(GLOBAL, 0x21, 1), exclusive(GLOBAL)]);
}

#[test]
fn try_leaves_the_other_word_untouched() {
    let sim = Sim::new().with_word(GLOBAL, 0x11);
    assert_eq!(try_acquire(&sim, LockWord::from_argument(EXPLICIT), LockStamp(0x21)), TryResult::Acquired);
    assert_eq!(sim.word(EXPLICIT), 0x21);
    assert_eq!(sim.word(GLOBAL), 0x11);
}

#[test]
fn release_clears_a_held_word_and_barriers_after_the_store() {
    let sim = Sim::new().with_word(GLOBAL, 0x11);
    assert_eq!(release(&sim, LockWord::DEFAULT_GLOBAL), ReleaseResult::Released);
    assert_eq!(sim.word(GLOBAL), 0);
    assert_eq!(sim.events(), vec![exclusive(GLOBAL), store(GLOBAL, 0, 0), BARRIER]);
}

#[test]
fn release_refuses_a_free_word_without_a_store_or_barrier() {
    let sim = Sim::new();
    assert_eq!(release(&sim, LockWord::DEFAULT_GLOBAL), ReleaseResult::AlreadyFree);
    assert_eq!(sim.word(GLOBAL), 0);
    assert_eq!(sim.events(), vec![exclusive(GLOBAL)]);
}

#[test]
fn release_clears_any_nonzero_word_without_an_owner_check() {
    let sim = Sim::new().with_word(EXPLICIT, 0xdead_beef);
    assert_eq!(release(&sim, LockWord::from_argument(EXPLICIT)), ReleaseResult::Released);
    assert_eq!(sim.word(EXPLICIT), 0);
}

#[test]
fn release_repeats_the_exclusive_pair_after_a_lost_store() {
    let sim = Sim::new().with_word(GLOBAL, 0x11);
    sim.lose_next_store();
    assert_eq!(release(&sim, LockWord::DEFAULT_GLOBAL), ReleaseResult::Released);
    assert_eq!(sim.word(GLOBAL), 0);
    assert_eq!(
        sim.events(),
        vec![exclusive(GLOBAL), store(GLOBAL, 0, 1), exclusive(GLOBAL), store(GLOBAL, 0, 0), BARRIER]
    );
}

#[test]
fn observe_reads_plainly_without_a_store_or_barrier() {
    let sim = Sim::new().with_word(GLOBAL, 0x22);
    assert_eq!(observe(&sim, LockWord::DEFAULT_GLOBAL), LockValue(0x22));
    assert_eq!(sim.events(), vec![Event::Load(GLOBAL)]);
    assert_eq!(sim.word(GLOBAL), 0x22);
}

#[test]
fn observe_returns_the_raw_word_including_zero_and_arbitrary_values() {
    let free = Sim::new();
    assert_eq!(observe(&free, LockWord::DEFAULT_GLOBAL), LockValue::FREE);
    assert!(observe(&free, LockWord::DEFAULT_GLOBAL).is_free());

    let held = Sim::new().with_word(EXPLICIT, 0xdead_beef);
    assert_eq!(observe(&held, LockWord::from_argument(EXPLICIT)).raw(), 0xdead_beef);
}

#[test]
fn the_three_services_report_the_stock_status_words() {
    assert_eq!(TryResult::Acquired.status(), 0);
    assert_eq!(TryResult::Held.status(), -1);
    assert_eq!(ReleaseResult::Released.status(), 0);
    assert_eq!(ReleaseResult::AlreadyFree.status(), -1);
}
