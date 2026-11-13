use xm1_saclac_profile_records::stock::shared::{group_14, group_27};

use crate::exposure::fakes::{handle, World};
use crate::exposure::snapshot;
use crate::stock::shared;

const REFERENCE: u32 = 0x55;

fn focus_bytes(reference: u32) -> [u8; 132] {
    let mut bytes = [0; 132];
    bytes[0..4].copy_from_slice(&reference.to_le_bytes());
    bytes[4..8].copy_from_slice(&9u32.to_le_bytes());
    bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
    for (index, term) in [100u16, 10, 20, 30, 400, 5].into_iter().enumerate() {
        bytes[12 + 2 * index..14 + 2 * index].copy_from_slice(&term.to_le_bytes());
    }
    bytes[0x60..0x64].copy_from_slice(&77u32.to_le_bytes());
    bytes
}

fn world(focus_reference: u32) -> World {
    let mut world = World::default();
    world.set_shared(&group_14::FOCUS_RESULT, focus_bytes(focus_reference));
    world.set_shared(&group_27::AT_0262, -4);
    world.lens_levels.insert((3, focus_reference), 7);
    world.put_u16(shared::focus_curve(0), 1000);
    world.put_u16(shared::focus_curve(255), 50);
    world
}

#[test]
fn a_focus_result_taken_at_the_bound_lens_reference_is_valid_and_lowers_the_focus_curve() {
    let mut world = world(REFERENCE);
    let snapshot = snapshot::take(&mut world, handle(), REFERENCE);
    assert_eq!((snapshot.valid, snapshot.extent), (9, 77));
    assert_eq!(snapshot.offset, 400 + 100 - 20 - 10 - 5 - 30 + 7);
    assert_eq!(snapshot.bounds[0], -4);
    assert_eq!(world.u16_at(shared::focus_curve(0)), 1000 - 60);
    assert_eq!(world.u16_at(shared::focus_curve(255)), 50u16.wrapping_sub(60));
    assert_eq!(world.stores.len(), 256);
}

#[test]
fn a_focus_result_taken_at_another_reference_is_invalid_and_stores_nothing() {
    let mut world = world(REFERENCE + 1);
    let snapshot = snapshot::take(&mut world, handle(), REFERENCE);
    assert_eq!((snapshot.valid, snapshot.extent, snapshot.offset), (0, 0, 0));
    assert_eq!(snapshot.focus.reference, REFERENCE + 1);
    assert!(world.stores.is_empty());
}
