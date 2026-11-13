use xm1_saclac_profile_records::stock::record::{group_00, group_13};
use xm1_saclac_profile_records::stock::shared::group_20;
use xm1_saclac_profile_records::{MeasurementSelection, SharedHeaderByte};

use crate::adjust::result_dispatch::*;
use crate::exposure::fakes::{handle, World};

fn result(world: &mut World) -> u32 { second_result(world, handle()).code() }

#[test]
fn a_selected_record_dispatches_by_its_kind_byte() {
    let mut world = World::default();
    world.select(MeasurementSelection::Record);
    world.set(&group_00::AT_0067, 1);
    assert_eq!(result(&mut world), 5);
    world.set(&group_00::AT_0034, 6);
    assert_eq!(result(&mut world), 4);
}

#[test]
fn an_unselected_record_dispatches_by_its_flag_and_class() {
    let mut world = World::default();
    assert_eq!(result(&mut world), 10);
    world.header(SharedHeaderByte::At47, 2);
    assert_eq!(result(&mut world), 3);
    world.set(&group_00::AT_0067, 1);
    assert_eq!(result(&mut world), 12);
}

fn statistics_word(world: &mut World) -> u32 { world.records.read_shared(handle(), &group_20::AT_014C).unwrap() }

#[test]
fn an_equal_result_is_unchanged_and_modes_three_and_four_refuse() {
    let mut world = World::default();
    world.set_shared(&group_20::AT_014C, 10);
    assert_eq!(adopt_result(&mut world, handle(), 0), Adoption::Unchanged);
    assert_eq!(adopt_result(&mut world, handle(), 3), Adoption::Refused);
    assert_eq!(adopt_result(&mut world, handle(), 4), Adoption::Refused);
}

#[test]
fn a_change_between_three_and_ten_is_adopted_unless_locked() {
    let mut world = World::default();
    world.set_shared(&group_20::AT_014C, 3);
    world.set(&group_13::AT_20B0_4, 3);
    assert_eq!(adopt_result(&mut world, handle(), 0), Adoption::Refused);
    assert_eq!(statistics_word(&mut world), 3);
    world.set(&group_13::AT_20B0_4, 0);
    assert_eq!(adopt_result(&mut world, handle(), 0), Adoption::Adopted);
    assert_eq!(statistics_word(&mut world), 10);
}

#[test]
fn any_other_change_is_refused() {
    let mut world = World::default();
    world.set_shared(&group_20::AT_014C, 5);
    assert_eq!(adopt_result(&mut world, handle(), 0), Adoption::Refused);
    assert_eq!(statistics_word(&mut world), 5);
}

struct State(u32);

impl ServiceState for State {
    fn service_state(&mut self) -> u32 { self.0 }
}

#[test]
fn the_dispatch_option_needs_a_class_no_hold_and_service_state_other_than_one() {
    let mut world = World::default();
    world.set(&group_00::AT_006E, 9);
    world.header(SharedHeaderByte::At47, 1);
    assert_eq!(dispatch_option(&mut world, &mut State(0), handle()), 9);
    assert_eq!(dispatch_option(&mut world, &mut State(1), handle()), 0);
    for class in [0, 4, 5, 6] {
        world.header(SharedHeaderByte::At47, class);
        assert_eq!(dispatch_option(&mut world, &mut State(0), handle()), 0);
    }
    world.header(SharedHeaderByte::At47, 7);
    world.set(&group_00::AT_0070, 1);
    assert_eq!(dispatch_option(&mut world, &mut State(0), handle()), 0);
}
