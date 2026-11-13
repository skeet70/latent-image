use xm1_saclac_profile_records::stock::record::group_00;
use xm1_saclac_profile_records::SharedHeaderByte;

use crate::exposure::fakes::{handle, World};
use crate::exposure::ports::Ports;

fn world(kind: u8, style: u8, flags: u16, withheld: bool) -> World {
    let mut world = World::default();
    world.header(SharedHeaderByte::At31, kind);
    world.header(SharedHeaderByte::At46, 9);
    world.header(SharedHeaderByte::At63, u8::from(withheld));
    world.set(&group_00::AT_003F, style);
    world.set(&group_00::AT_002E, flags);
    world
}

#[test]
fn a_record_that_withholds_its_policies_reads_zero_on_the_refinement_pass_only() {
    let mut world = world(5, 1, 0x12, true);
    let mut profile = world.profile();
    assert_eq!((profile.selection_kind(handle(), false), profile.selection_flags(handle(), false), profile.selection_auxiliary(handle(), false)), (5, 0x12, 9));
    assert_eq!((profile.selection_kind(handle(), true), profile.selection_flags(handle(), true), profile.selection_auxiliary(handle(), true)), (0, 0, 0));
    let mut open = self::world(5, 1, 0x12, false);
    assert_eq!(open.profile().selection_kind(handle(), true), 5);
}

#[test]
fn the_style_of_a_record_without_the_withholding_byte_is_the_record_style() {
    let mut world = world(5, 1, 2, false);
    assert_eq!(world.profile().selection_style(handle(), false), 1);
}

#[test]
fn a_withholding_record_narrows_the_style_for_listed_kinds() {
    assert_eq!(world(5, 1, 0, true).profile().selection_style(handle(), false), 0);
    assert_eq!(world(48, 3, 0, true).profile().selection_style(handle(), false), 2);
    assert_eq!(world(6, 1, 0, true).profile().selection_style(handle(), false), 1);
    assert_eq!(world(5, 4, 0, true).profile().selection_style(handle(), false), 4);
}

#[test]
fn flag_bit_one_makes_a_withholding_record_style_six() {
    assert_eq!(world(6, 1, 2, true).profile().selection_style(handle(), false), 6);
    assert_eq!(world(6, 1, 2, true).profile().selection_style(handle(), true), 1);
}

#[test]
fn style_seven_or_option_one_gives_option_zero() {
    assert_eq!(world(6, 7, 0, false).profile().style_option(handle(), 0), 0);
    assert_eq!(world(6, 2, 0, false).profile().style_option(handle(), 1), 0);
    assert_eq!(world(6, 2, 0, false).profile().style_option(handle(), 2), 4);
}
