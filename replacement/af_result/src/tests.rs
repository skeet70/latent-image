use super::{release, reserve, rotate, select, Address, Bus, Group, Pool, Slot};
use std::collections::{BTreeMap, VecDeque};
use std::vec::Vec;

const MODE: u32 = 0x016f_ec4c;
const GROUP0: u32 = 0x016f_edd0;
const GROUP1: u32 = 0x016f_ed28;
const ALTERNATE: u32 = 0x0170_2f84;
const MARK: u32 = 0x0170_306c;
const CURRENT: u32 = 0x0170_3080;
const COMPLETED: u32 = 0x0170_3084;
const POLICY: u32 = 0x0170_319c;
const TOKEN: u32 = 0xa735_106c;

#[derive(Default)]
struct Memory {
    words: BTreeMap<u32, u32>,
    changing: BTreeMap<u32, VecDeque<u32>>,
    writes: Vec<(u32, u32)>,
    guarded: bool,
    inside: bool,
    entries: usize,
    exits: usize,
}

impl Memory {
    fn pool(base: u32, states: &[u32]) -> Self {
        let mut memory = Self::default();
        for (slot, &state) in states.iter().enumerate() {
            let record = base + slot as u32 * 0x18;
            for offset in [0, 8, 12, 20] {
                memory.words.insert(record + offset, 0xace0_0000 + record + offset);
            }
            memory.words.insert(record + 4, state);
            memory.words.insert(record + 16, 0);
        }
        memory
    }

    fn set(&mut self, address: u32, value: u32) {
        self.words.insert(address, value);
    }

    fn changing(&mut self, address: u32, values: &[u32]) {
        self.changing.insert(address, values.iter().copied().collect());
    }

    fn expect_states(&self, before: &BTreeMap<u32, u32>, base: u32, states: &[u32]) {
        let mut expected = before.clone();
        for (slot, &state) in states.iter().enumerate() {
            expected.insert(base + slot as u32 * 0x18 + 4, state);
        }
        assert_eq!(self.words, expected);
        for &(address, _) in &self.writes {
            assert!((0..states.len()).any(|slot| address == base + slot as u32 * 0x18 + 4));
        }
    }

    fn expect_guard_released(&self, calls: usize) {
        assert!(!self.inside);
        assert_eq!(self.entries, calls);
        assert_eq!(self.exits, calls);
    }
}

impl Bus for Memory {
    fn read_word(&mut self, address: Address) -> u32 {
        assert!(!self.guarded || self.inside);
        if let Some(value) = self.changing.get_mut(&address.0).and_then(VecDeque::pop_front) {
            return value;
        }
        self.words.get(&address.0).copied().unwrap_or(0)
    }

    fn write_word(&mut self, address: Address, value: u32) {
        assert!(!self.guarded || self.inside);
        self.words.insert(address.0, value);
        self.writes.push((address.0, value));
    }

    fn enter(&mut self) -> u32 {
        assert!(!self.inside);
        self.inside = true;
        self.entries += 1;
        TOKEN
    }

    fn leave(&mut self, token: u32) {
        assert!(self.inside);
        assert_eq!(token, TOKEN);
        self.inside = false;
        self.exits += 1;
    }
}

#[test]
fn selector_uses_only_raw_one_for_group_one() {
    for mode in [0, 4, 5, 6, 7, u32::MAX] {
        for raw in [0, 1, 2, u32::MAX] {
            let mut memory = Memory::default();
            memory.set(MODE, mode);
            let before = memory.words.clone();
            let pool = select(&mut memory, Group::from_raw(raw));
            let (base, count) = match (raw, mode) {
                (1, _) => (GROUP1, 7),
                (_, 5 | 6) => (ALTERNATE, 4),
                _ => (GROUP0, 5),
            };
            assert_eq!(pool.base.0, base);
            assert_eq!(pool.count, count);
            assert_eq!(memory.words, before);
            assert!(memory.writes.is_empty());
            memory.expect_guard_released(0);
        }
    }
}

#[test]
fn any_reservation_chooses_first_free_in_each_selected_pool() {
    for (raw, mode, base, states, expected) in [
        (0, 0, GROUP0, &[0x91, 0xd, 0, 0, 0][..], &[0x91, 0xd, 1, 0, 0][..]),
        (1, 5, GROUP1, &[0x91, 0xd, 0, 0, 0, 0, 0][..], &[0x91, 0xd, 1, 0, 0, 0, 0][..]),
        (2, 5, ALTERNATE, &[0x91, 0xd, 0, 0][..], &[0x91, 0xd, 1, 0][..]),
        (0, 6, ALTERNATE, &[0x91, 0xd, 0, 0][..], &[0x91, 0xd, 1, 0][..]),
    ] {
        let mut memory = Memory::pool(base, states);
        memory.set(MODE, mode);
        memory.guarded = true;
        let before = memory.words.clone();
        assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(raw)).0, 2);
        memory.expect_states(&before, base, expected);
        memory.expect_guard_released(1);
    }
}

#[test]
fn any_reservation_reuses_first_pending_before_any_free_slot() {
    let mut memory = Memory::pool(GROUP0, &[0, 0x1b, 1, 1, 0]);
    memory.guarded = true;
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 2);
    memory.expect_states(&before, GROUP0, &[0, 0x1b, 1, 1, 0]);
    assert!(memory.writes.is_empty());
    memory.expect_guard_released(1);
}

#[test]
fn occupied_and_unknown_states_are_not_free() {
    for state in [1, 2, 3, 0xd, 0x1b, 0x83, u32::MAX] {
        let mut memory = Memory::pool(GROUP0, &[state; 5]);
        memory.guarded = true;
        let before = memory.words.clone();
        let result = reserve(&mut memory, Slot(3), Group::from_raw(0));
        assert_eq!(result.0, Slot::ANY.0);
        memory.expect_states(&before, GROUP0, &[state; 5]);
        assert!(memory.writes.is_empty());
        memory.expect_guard_released(1);
    }
}

#[test]
fn full_pool_without_pending_returns_any_without_rewriting_records() {
    let mut memory = Memory::pool(GROUP0, &[2, 3, 0xd, 0x1b, 0x1234]);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, Slot::ANY.0);
    memory.expect_states(&before, GROUP0, &[2, 3, 0xd, 0x1b, 0x1234]);
    assert!(memory.writes.is_empty());
    memory.expect_guard_released(1);
}

#[test]
fn special_mark_is_exactly_one_and_does_not_change_record_payload() {
    for (mark, expected) in [(0, 1), (1, 2), (2, 1), (u32::MAX, 1)] {
        let mut memory = Memory::pool(GROUP0, &[0, 0, 0, 0, 0]);
        memory.set(MARK, mark);
        memory.set(GROUP0 + 0x18 + 0x10, 0xffff_ffff);
        let before = memory.words.clone();
        assert_eq!(reserve(&mut memory, Slot(1), Group::from_raw(0)).0, 1);
        memory.expect_states(&before, GROUP0, &[0, expected, 0, 0, 0]);
        memory.expect_guard_released(1);
    }
}

#[test]
fn any_reservation_honors_special_mark() {
    let mut memory = Memory::pool(GROUP1, &[0xd, 0, 0, 0, 0, 0, 0]);
    memory.set(MARK, 1);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(1)).0, 1);
    memory.expect_states(&before, GROUP1, &[0xd, 2, 0, 0, 0, 0, 0]);
}

#[test]
fn explicit_reservation_does_not_reuse_a_different_pending_slot() {
    let mut memory = Memory::pool(GROUP0, &[1, 3, 0, 0, 0]);
    memory.set(CURRENT, 10);
    memory.set(COMPLETED, 0);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot(2), Group::from_raw(0)).0, 2);
    memory.expect_states(&before, GROUP0, &[1, 3, 1, 0, 0]);
}

#[test]
fn explicit_slot_preserves_stock_unchecked_addressing() {
    let mut memory = Memory::pool(GROUP0, &[2, 2, 2, 2, 2, 2, 0]);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot(6), Group::from_raw(0)).0, 6);
    memory.expect_states(&before, GROUP0, &[2, 2, 2, 2, 2, 2, 1]);
}

#[test]
fn reservations_stop_at_selected_pool_boundary() {
    for (raw, mode, base, states) in [
        (0, 0, GROUP0, &[2, 2, 2, 2, 2, 0][..]),
        (1, 0, GROUP1, &[2, 2, 2, 2, 2, 2, 2, 0][..]),
        (0, 5, ALTERNATE, &[2, 2, 2, 2, 0][..]),
    ] {
        let mut memory = Memory::pool(base, states);
        memory.set(MODE, mode);
        let before = memory.words.clone();
        assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(raw)).0, Slot::ANY.0);
        memory.expect_states(&before, base, states);
        assert!(memory.writes.is_empty());
    }
}

#[test]
fn reserve_counter_equality_and_single_step_lag_reuse_pending() {
    for (completed, current) in [(9, 9), (9, 10), (10, 9), (u32::MAX - 1, u32::MAX)] {
        let mut memory = Memory::pool(GROUP0, &[3, 1, 0, 0, 0]);
        memory.set(COMPLETED, completed);
        memory.set(CURRENT, current);
        let before = memory.words.clone();
        assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 1);
        memory.expect_states(&before, GROUP0, &[3, 1, 0, 0, 0]);
        assert!(memory.writes.is_empty());
    }
}

#[test]
fn late_reservation_retires_first_ready_each_pass_and_marks_pending() {
    let mut memory = Memory::pool(GROUP0, &[3, 1, 0, 3, 0]);
    memory.set(COMPLETED, 7);
    memory.set(CURRENT, 9);
    memory.set(GROUP0 + 0x10, 1);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 0);
    memory.expect_states(&before, GROUP0, &[1, 0x1b, 0, 0, 0]);
}

#[test]
fn late_reservation_without_pending_clears_all_ready_before_allocating() {
    let mut memory = Memory::pool(GROUP0, &[0x77, 3, 3, 2, 0xd]);
    memory.set(COMPLETED, 0);
    memory.set(CURRENT, 2);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 1);
    memory.expect_states(&before, GROUP0, &[0x77, 1, 0, 2, 0xd]);
}

#[test]
fn failed_late_reservation_keeps_the_pending_mark() {
    let mut memory = Memory::pool(GROUP0, &[1, 2, 0xd, 0x1b, 0x99]);
    memory.set(COMPLETED, 0);
    memory.set(CURRENT, 2);
    memory.guarded = true;
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, Slot::ANY.0);
    memory.expect_states(&before, GROUP0, &[0x1b, 2, 0xd, 0x1b, 0x99]);
    memory.expect_guard_released(1);
}

#[test]
fn reserve_counter_increment_wraps_before_unsigned_comparison() {
    for (current, expected_slot, expected) in [
        (0, 0, [1, 0, 0, 0, 0]),
        (1, 1, [0x1b, 1, 0, 0, 0]),
    ] {
        let mut memory = Memory::pool(GROUP0, &[1, 0, 0, 0, 0]);
        memory.set(COMPLETED, u32::MAX);
        memory.set(CURRENT, current);
        let before = memory.words.clone();
        assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, expected_slot);
        memory.expect_states(&before, GROUP0, &expected);
    }
}

#[test]
fn reservation_observes_counters_again_for_each_candidate() {
    let mut memory = Memory::pool(GROUP0, &[0x55, 1, 0, 0, 0]);
    memory.set(COMPLETED, 0);
    memory.set(CURRENT, 2);
    memory.changing(CURRENT, &[0, 2]);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 2);
    memory.expect_states(&before, GROUP0, &[0x55, 0x1b, 1, 0, 0]);
}

#[test]
fn reservation_observes_completed_counter_again_for_each_candidate() {
    let mut memory = Memory::pool(GROUP0, &[0x55, 1, 0, 0, 0]);
    memory.set(CURRENT, 2);
    memory.set(COMPLETED, 0);
    memory.changing(COMPLETED, &[2, 0]);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 2);
    memory.expect_states(&before, GROUP0, &[0x55, 0x1b, 1, 0, 0]);
}

#[test]
fn failed_second_reservation_does_not_roll_back_first() {
    let mut memory = Memory::pool(GROUP0, &[0, 2, 0xd, 0x1b, 0x99]);
    memory.guarded = true;
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot(0), Group::from_raw(0)).0, 0);
    assert_eq!(reserve(&mut memory, Slot(1), Group::from_raw(0)).0, Slot::ANY.0);
    memory.expect_states(&before, GROUP0, &[1, 2, 0xd, 0x1b, 0x99]);
    memory.expect_guard_released(2);
}

#[test]
fn rotation_retires_first_active_and_first_ready_only() {
    let mut memory = Memory::pool(GROUP0, &[0x77, 3, 2, 3, 2]);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[0x77, 0, 0, 3, 2]);
    memory.expect_guard_released(0);
}

#[test]
fn rotation_handles_first_slot_ready_or_active() {
    for (states, expected) in [
        ([3, 0x77, 2, 3, 2], [0, 0x77, 0, 3, 2]),
        ([2, 0x77, 3, 2, 3], [0, 0x77, 0, 2, 3]),
    ] {
        let mut memory = Memory::pool(GROUP0, &states);
        let before = memory.words.clone();
        rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
        memory.expect_states(&before, GROUP0, &expected);
    }
}

#[test]
fn rotation_preserves_nonzero_per_record_flag_and_exact_policy() {
    for (policy, active_state) in [(0, 0), (1, 3), (2, 0), (u32::MAX, 0)] {
        for (flag, ready_state) in [(0, 0), (1, 0xd), (u32::MAX, 0xd)] {
            let mut memory = Memory::pool(GROUP0, &[3, 2, 0x77, 0xd, 0x1b]);
            memory.set(POLICY, policy);
            memory.set(GROUP0 + 0x10, flag);
            let before = memory.words.clone();
            rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
            memory.expect_states(&before, GROUP0, &[ready_state, active_state, 0x77, 0xd, 0x1b]);
        }
    }
}

#[test]
fn rotation_without_a_ready_slot_does_not_retire_newly_ready_active_again() {
    let mut memory = Memory::pool(GROUP0, &[2, 0x77, 2, 0xd, 0x1b]);
    memory.set(POLICY, 1);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[3, 0x77, 2, 0xd, 0x1b]);
}

#[test]
fn rotation_without_active_still_retires_first_ready() {
    let mut memory = Memory::pool(GROUP0, &[0x77, 3, 0, 3, 0xd]);
    memory.set(GROUP0 + 0x18 + 0x10, 7);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[0x77, 0xd, 0, 3, 0xd]);
}

#[test]
fn rotation_promotes_first_pending_for_equal_or_ahead_counters() {
    for (completed, current) in [(5, 5), (6, 5), (u32::MAX, 0)] {
        let mut memory = Memory::pool(GROUP0, &[0x1b, 1, 1, 0x1b, 0x77]);
        memory.set(COMPLETED, completed);
        memory.set(CURRENT, current);
        let before = memory.words.clone();
        rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
        memory.expect_states(&before, GROUP0, &[0x1b, 2, 1, 0x1b, 0x77]);
    }
}

#[test]
fn rotation_promotes_first_late_pending_for_behind_counters() {
    for (completed, current) in [(5, 6), (0, u32::MAX)] {
        let mut memory = Memory::pool(GROUP0, &[1, 0x1b, 0x1b, 1, 0x77]);
        memory.set(COMPLETED, completed);
        memory.set(CURRENT, current);
        let before = memory.words.clone();
        rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
        memory.expect_states(&before, GROUP0, &[1, 2, 0x1b, 1, 0x77]);
    }
}

#[test]
fn rotation_uses_one_current_counter_snapshot_for_promotion() {
    let mut memory = Memory::pool(GROUP0, &[0x77, 0x1b, 1, 0, 0]);
    memory.set(COMPLETED, 0);
    memory.set(CURRENT, 2);
    memory.changing(CURRENT, &[0, 2]);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[0x77, 0x1b, 2, 0, 0]);
}

#[test]
fn rotation_uses_one_completed_counter_snapshot_for_promotion() {
    let mut memory = Memory::pool(GROUP0, &[0x77, 0x1b, 1, 0, 0]);
    memory.set(CURRENT, 2);
    memory.set(COMPLETED, 0);
    memory.changing(COMPLETED, &[2, 0]);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[0x77, 0x1b, 2, 0, 0]);
}

#[test]
fn rotation_without_matching_states_preserves_all_words() {
    let mut memory = Memory::pool(GROUP0, &[0, 0xd, 0x1b, 0x77, u32::MAX]);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[0, 0xd, 0x1b, 0x77, u32::MAX]);
    assert!(memory.writes.is_empty());
}

#[test]
fn empty_rotation_has_no_bus_effects() {
    let mut memory = Memory::default();
    memory.guarded = true;
    rotate(&mut memory, Pool { base: Address(0xffff_fff0), count: 0 });
    assert!(memory.words.is_empty());
    assert!(memory.writes.is_empty());
    memory.expect_guard_released(0);
}

#[test]
fn single_record_rotation_retires_then_promotes_without_crossing_boundary() {
    for (state, expected) in [(0, 0), (1, 2), (2, 3), (3, 0xd), (0x77, 0x77)] {
        let mut memory = Memory::pool(GROUP0, &[state, 3]);
        memory.set(POLICY, 1);
        memory.set(GROUP0 + 0x10, 1);
        let before = memory.words.clone();
        rotate(&mut memory, Pool { base: Address(GROUP0), count: 1 });
        memory.expect_states(&before, GROUP0, &[expected, 3]);
    }
}

#[test]
fn release_selects_pool_and_keeps_entire_transition_in_critical_section() {
    for (raw, mode, base, states, expected) in [
        (0, 0, GROUP0, &[3, 2, 1, 1, 0][..], &[0, 0, 2, 1, 0][..]),
        (1, 6, GROUP1, &[3, 2, 1, 1, 0, 3, 2][..], &[0, 0, 2, 1, 0, 3, 2][..]),
        (2, 5, ALTERNATE, &[3, 2, 1, 1][..], &[0, 0, 2, 1][..]),
        (0, 6, ALTERNATE, &[3, 2, 1, 1][..], &[0, 0, 2, 1][..]),
    ] {
        let mut memory = Memory::pool(base, states);
        memory.set(MODE, mode);
        memory.guarded = true;
        let before = memory.words.clone();
        release(&mut memory, Group::from_raw(raw));
        memory.expect_states(&before, base, expected);
        memory.expect_guard_released(1);
    }
}

#[test]
fn successive_reserve_release_cycles_preserve_payload_and_ownership() {
    let mut memory = Memory::pool(GROUP0, &[0, 0, 0, 0, 0]);
    memory.guarded = true;
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 0);
    release(&mut memory, Group::from_raw(0));
    memory.expect_states(&before, GROUP0, &[2, 0, 0, 0, 0]);
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 1);
    release(&mut memory, Group::from_raw(0));
    memory.expect_states(&before, GROUP0, &[0, 2, 0, 0, 0]);
    release(&mut memory, Group::from_raw(0));
    memory.expect_states(&before, GROUP0, &[0, 0, 0, 0, 0]);
    memory.expect_guard_released(5);
}

#[test]
fn explicit_slot_keeps_full_width_index_and_wrapping_record_address() {
    for (requested, record) in [
        (0x0001_0006, GROUP0 + 0x0018_0090),
        (0x2000_0006, GROUP0 + 0x90),
    ] {
        let mut memory = Memory::pool(GROUP0, &[2, 2, 2, 2, 2]);
        memory.set(record + 4, 0);
        memory.set(record + 0x10, 0x7654_3210);
        let before = memory.words.clone();
        assert_eq!(reserve(&mut memory, Slot(requested), Group::from_raw(0)).0, requested);
        memory.expect_states(&before, record, &[1]);
        memory.expect_guard_released(1);
    }
}

#[test]
fn special_mark_does_not_rewrite_an_existing_pending_reservation() {
    let mut memory = Memory::pool(GROUP0, &[0, 1, 0, 0, 0]);
    memory.set(MARK, 1);
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot::ANY, Group::from_raw(0)).0, 1);
    memory.expect_states(&before, GROUP0, &[0, 1, 0, 0, 0]);
    assert!(memory.writes.is_empty());
}

#[test]
fn failed_first_reservation_does_not_prevent_second_acquisition() {
    let mut memory = Memory::pool(GROUP0, &[2, 0, 0xd, 0x1b, 0x99]);
    memory.guarded = true;
    let before = memory.words.clone();
    assert_eq!(reserve(&mut memory, Slot(0), Group::from_raw(0)).0, Slot::ANY.0);
    assert_eq!(reserve(&mut memory, Slot(1), Group::from_raw(0)).0, 1);
    memory.expect_states(&before, GROUP0, &[2, 1, 0xd, 0x1b, 0x99]);
    memory.expect_guard_released(2);
}

#[test]
fn rotation_reuses_first_record_state_across_ready_and_active_searches() {
    let mut memory = Memory::pool(GROUP0, &[0, 3, 2, 0x77, 0]);
    memory.changing(GROUP0 + 4, &[0, 2]);
    let before = memory.words.clone();
    rotate(&mut memory, Pool { base: Address(GROUP0), count: 5 });
    memory.expect_states(&before, GROUP0, &[0, 0, 0, 0x77, 0]);
}
