use super::*;

/// A returned retained call; a commit-family entry heads its commit on engine one.
fn call(entry: Entry) -> Call {
    let engine = entry.committed_engine(&[1]);
    Call {
        entry, caller: Caller::Retained, head: engine.is_some(), engine, arguments: Vec::new(), result: None, selection: [1, 0], slots: Vec::new(),
        pending: 0, front: None, saved: Vec::new(), before_drain: 0, bus: Vec::new(), callbacks: Vec::new(), changes: Vec::new(), item: None, returned: true,
    }
}

fn saved_image() -> Vec<u32> { (0..0x1a4 / 4).map(|word| 0x5100_0000 | word).collect() }

/// A pending engine-one commit that wrote `registers` with the matching saved words, then drained nothing.
fn commit_writing(registers: &[(u32, u32)]) -> Call {
    let saved = saved_image();
    let bus = registers.iter().map(|(source, register)| Bus { address: 0xfff6e000 + register, write: true, value: saved[*source as usize / 4] }).collect::<Vec<_>>();
    Call { arguments: vec![1], pending: 1, slots: vec![0; 12], saved, before_drain: bus.len(), bus, ..call(Entry::Commit) }
}

fn drained(slots: [u32; 12], callbacks: Vec<Callback>) -> Call {
    Call { arguments: vec![1], slots: slots.to_vec(), callbacks, ..call(Entry::Commit) }
}

fn ran(target: u32, live: [u32; 12], after: [u32; 12]) -> Callback { Callback { target, live: live.to_vec(), after: Some(after.to_vec()) } }

#[test]
fn offset_map_matches_the_contract_shape() {
    let one = register_map(1);
    let zero = register_map(0);
    assert_eq!(one.len(), 105);
    assert_eq!(zero.len(), 104);
    assert!(!zero.iter().any(|(_, register)| *register == 0x18));
    assert_eq!(&one[..8], &[(0, 0), (4, 4), (8, 8), (0xc, 0xc), (0x10, 0x10), (0x14, 0x14), (0x18, 0x18), (0x1c, 0x1c)]);
    let groups: Vec<u32> = one.iter().map(|(_, register)| *register).skip_while(|register| *register != 0x44).take(9).collect();
    assert_eq!(groups, [0x44, 0x54, 0x64, 0x74, 0x94, 0xa4, 0xb4, 0xc4, 0x48]);
    let position = |register: u32| one.iter().position(|(_, value)| *value == register).unwrap();
    assert_eq!(one[position(0x108)].0, 0xe8);
    assert!(position(0x190) < position(0x108) && position(0x108) < position(0xe8));
    let mut registers: Vec<u32> = one.iter().map(|(_, register)| *register).collect();
    registers.sort_unstable();
    registers.dedup();
    assert_eq!(registers.len(), one.len());
}

#[test]
fn a_commit_writing_the_saved_image_in_map_order_is_lawful() {
    assert_eq!(laws(&[commit_writing(&register_map(1))]), []);
}

#[test]
fn reordered_registers_are_refused_at_their_position() {
    let mut map = register_map(1);
    map.swap(0, 1);
    assert!(matches!(laws(&[commit_writing(&map)]).as_slice(), [Finding::RegisterSequence { position: 0, .. }]));
}

#[test]
fn independent_group_runs_are_refused() {
    let mut map = register_map(1);
    let start = map.iter().position(|(_, register)| *register == 0x44).unwrap();
    map[start..start + 32].sort_by_key(|(_, register)| *register);
    assert!(matches!(laws(&[commit_writing(&map)]).as_slice(), [Finding::RegisterSequence { .. }]));
}

#[test]
fn a_missing_or_wrong_value_is_refused() {
    let mut short = commit_writing(&register_map(1));
    short.bus.pop();
    short.before_drain -= 1;
    assert!(matches!(laws(&[short]).as_slice(), [Finding::RegisterSequence { position: 104, observed: None, .. }]));
    let mut stale = commit_writing(&register_map(1));
    stale.bus[3].value ^= 1;
    assert!(matches!(laws(&[stale]).as_slice(), [Finding::RegisterSequence { position: 3, .. }]));
}

#[test]
fn writes_after_the_drain_starts_are_not_commit_writes() {
    let mut late = commit_writing(&register_map(1));
    late.bus.push(Bus { address: 0xfff6e000, write: true, value: 7 });
    assert_eq!(laws(&[late]), []);
}

#[test]
fn an_unpending_commit_must_not_write_registers() {
    let quiet = Call { arguments: vec![1], slots: vec![0; 12], bus: vec![Bus { address: 0xfff6e0b4, write: true, value: 1 }], before_drain: 1, ..call(Entry::Commit) };
    assert_eq!(laws(&[quiet]), [Finding::UnpendingWrite { call: 0 }]);
}

#[test]
fn both_lists_drain_in_slot_order_with_each_slot_cleared_first() {
    let entry = [0xa, 0, 0xb, 0, 0, 0, 0xc, 0, 0, 0, 0, 0];
    let callbacks = vec![
        ran(0xa, [0, 0, 0xb, 0, 0, 0, 0xc, 0, 0, 0, 0, 0], [0, 0, 0xb, 0, 0, 0, 0xc, 0, 0, 0, 0, 0]),
        ran(0xb, [0, 0, 0, 0, 0, 0, 0xc, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0xc, 0, 0, 0, 0, 0]),
        ran(0xc, [0; 12], [0; 12]),
    ];
    assert_eq!(laws(&[drained(entry, callbacks)]), []);
}

#[test]
fn a_skipped_second_list_is_refused() {
    let entry = [0xa, 0, 0, 0, 0, 0, 0xc, 0, 0, 0, 0, 0];
    let callbacks = vec![ran(0xa, [0, 0, 0, 0, 0, 0, 0xc, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0, 0xc, 0, 0, 0, 0, 0])];
    assert!(matches!(laws(&[drained(entry, callbacks)]).as_slice(), [Finding::Drain { callback: None, .. }]));
}

#[test]
fn a_callback_entered_before_its_slot_was_cleared_is_refused() {
    let entry = [0xa, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    assert!(matches!(laws(&[drained(entry, vec![ran(0xa, entry, [0; 12])])]).as_slice(), [Finding::Drain { callback: Some(0), .. }]));
}

#[test]
fn second_list_before_first_is_refused() {
    let entry = [0xa, 0, 0, 0, 0, 0, 0xc, 0, 0, 0, 0, 0];
    let callbacks = vec![ran(0xc, [0xa, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], [0xa, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]), ran(0xa, [0; 12], [0; 12])];
    assert!(matches!(laws(&[drained(entry, callbacks)]).as_slice(), [Finding::Drain { callback: Some(0), .. }]));
}

#[test]
fn a_slot_set_by_an_earlier_callback_is_drained_when_reached() {
    let entry = [0xa, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let requeued = [0, 0, 0, 0xd, 0, 0, 0, 0, 0, 0, 0, 0];
    let lawful = vec![ran(0xa, [0; 12], requeued), ran(0xd, [0; 12], [0; 12])];
    assert_eq!(laws(&[drained(entry, lawful)]), []);
    let missed = vec![ran(0xa, [0; 12], requeued)];
    assert!(matches!(laws(&[drained(entry, missed)]).as_slice(), [Finding::Drain { callback: None, .. }]));
}

fn publication(engine: u32, value: u32, before: u32, after: u32) -> Call {
    Call { caller: Caller::Vglib, arguments: vec![engine, 1, 0, value], result: Some(0), item: Some([before, after]), ..call(Entry::ItemConfiguration) }
}

#[test]
fn vglib_publication_targets_the_selected_engine() {
    assert_eq!(laws(&[publication(1, 0x4228c800, 0xc000_0000, 0xc228c800)]), []);
    assert_eq!(laws(&[Call { selection: [0, 0], ..publication(1, 0x4228c800, 0xc000_0000, 0xc228c800) }]),
        [Finding::Selection { call: 0, engine: 1, selected: 0 }]);
}

#[test]
fn the_item_setter_keeps_the_top_bits_and_refuses_large_items() {
    assert_eq!(laws(&[publication(1, 0x4228c800, 0x8000_0001, 0x8228c800)]), []);
    assert_eq!(laws(&[publication(1, 0x4228c800, 0x8000_0001, 0x4228c800)]), [Finding::ItemWord { call: 0 }]);
    let large = Call { arguments: vec![1, 1, 4, 0], result: Some((-30i32) as u32), ..call(Entry::ItemConfiguration) };
    assert_eq!(laws(&[large.clone()]), []);
    assert_eq!(laws(&[Call { result: Some(0), ..large }]), [Finding::ItemWord { call: 0 }]);
}

#[test]
fn shadow_must_follow_the_front_byte_or_the_last_publication() {
    let at = |shadow: u32| Call { arguments: vec![1], slots: vec![0; 12], front: Some(FrontState { front: 1, image_address: 0x423de000, shadow }), ..call(Entry::Commit) };
    assert_eq!(laws(&[at(0x023de000)]), []);
    assert!(matches!(laws(&[at(0x0228c800)]).as_slice(), [Finding::FrontStep { .. }]));
    assert_eq!(laws(&[publication(1, 0x4228c800, 0, 0x0228c800), at(0x0228c800)]), []);
}

#[test]
fn the_comparison_ignores_calls_the_unit_makes_into_itself() {
    let inner = Call { caller: Caller::Unit, head: false, ..call(Entry::Commit) };
    let outer = call(Entry::CommitOne);
    assert_eq!(first_difference(&[outer.clone(), inner], &[outer.clone()]), None);
    let changed = Call { bus: vec![Bus { address: 0xfff6e0b4, write: true, value: 2 }], ..outer.clone() };
    assert_eq!(first_difference(&[outer.clone()], &[changed]).map(|difference| difference.boundary_call), Some(0));
}

#[test]
fn a_commit_the_unit_heads_is_compared() {
    let persistent = Call { caller: Caller::Unit, ..call(Entry::CommitZero) };
    let changed = Call { pending: 1, ..persistent.clone() };
    assert!(first_difference(&[persistent], &[changed]).is_some());
}

#[test]
fn unit_detection_covers_the_retired_bodies_and_the_owned_range() {
    let body = (0xba0020, 0xba4000);
    assert_eq!(Caller::of(0x6ca500, body), Caller::Unit);
    assert_eq!(Caller::of(0xba1000, body), Caller::Unit);
    assert_eq!(Caller::of(0x6d0408, body), Caller::Vglib);
    assert_eq!(Caller::of(0x6d7818, body), Caller::Retained);
    assert_eq!(Caller::of(0x72b680, body), Caller::Retained);
}

#[test]
fn void_results_are_not_compared() {
    let stock = Call { result: Some(7), ..call(Entry::CommitOne) };
    let owned = Call { result: Some(0), ..call(Entry::CommitOne) };
    assert_eq!(first_difference(&[stock], &[owned]), None);
    let stock = Call { result: Some(0), ..call(Entry::Capture) };
    let owned = Call { result: Some(1), ..call(Entry::Capture) };
    assert!(first_difference(&[stock], &[owned]).is_some());
}
