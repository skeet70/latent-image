use super::fake::FakeCapture;
use super::gains::{blend_weight, gain_class, gains, GainClass, GainRequest};
use super::ports::Reference;
use super::ProfileKind;

fn sources() -> FakeCapture { FakeCapture::new(ProfileKind::Untimed, Reference::default()) }

#[test]
fn a_plain_variant_takes_its_table_class() {
    let mut ports = sources();
    ports.class_gains.push((GainClass(7), [300, 256, 410]));
    let request = GainRequest { variant: 3, ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &request), ([300, 256, 410], GainClass(7)));
}

#[test]
fn a_preset_takes_the_class_its_sensitivity_entry_names() {
    let mut ports = sources();
    ports.set_byte(0x793d, 2);
    let request = GainRequest { preset: 3, ..GainRequest::default() };
    assert_eq!(gain_class(&mut ports, &request), GainClass(2));
    ports.set_byte(0x7941, 4);
    let second_bank = GainRequest { bank: 1, ..request };
    assert_eq!(gain_class(&mut ports, &second_bank), GainClass(4));
}

#[test]
fn a_preset_whose_entry_names_no_class_takes_the_preset_table() {
    let mut ports = sources();
    let request = GainRequest { preset: 3, ..GainRequest::default() };
    let (_, class) = gains(&mut ports, &request);
    assert_eq!(class, GainClass(3));
    assert_eq!(ports.gain_calls, [GainClass(3)]);
}

#[test]
fn preset_two_maps_scene_classes_by_mode() {
    let mut ports = sources();
    ports.set_byte(0x793c, 2);
    let request = GainRequest { preset: 2, mode: 18, ..GainRequest::default() };
    assert_eq!(gain_class(&mut ports, &request), GainClass(29));
    let other_mode = GainRequest { mode: 19, ..request };
    assert_eq!(gain_class(&mut ports, &other_mode), GainClass(2));
}

#[test]
fn the_variant_set_moves_a_preset_class_into_its_range() {
    let mut ports = sources();
    ports.set_byte(0x793e, 3);
    ports.set_byte(0x7ccf, 1);
    let request = GainRequest { preset: 4, variant_set: 3, ..GainRequest::default() };
    assert_eq!(gain_class(&mut ports, &request), GainClass(21));
}

#[test]
fn a_disabled_variant_set_forces_the_first_preset() {
    let mut ports = sources();
    ports.set_byte(0x793b, 4);
    let request = GainRequest { preset: 3, variant_set: 1, ..GainRequest::default() };
    assert_eq!(gain_class(&mut ports, &request), GainClass(4));
}

#[test]
fn the_unpreset_blend_runs_between_the_cold_and_warm_tables() {
    let mut ports = sources();
    ports.class_gains.push((GainClass(18), [200, 256, 600]));
    ports.class_gains.push((GainClass(5), [400, 256, 300]));
    ports.set_byte(0x791e, 30);
    ports.set_byte(0x791f, 60);
    ports.temperature_offset = 50;
    let at = |temperature| GainRequest { temperature, ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &at(400)), ([300, 256, 450], GainClass(0)));
    assert_eq!(gains(&mut ports, &at(250)).0, [200, 256, 600]);
    assert_eq!(gains(&mut ports, &at(550)).0, [400, 256, 300]);
}

#[test]
fn a_complete_request_triple_replaces_the_blend() {
    let mut ports = sources();
    let request = GainRequest { custom: [1, 2, 3], ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &request), ([1, 2, 3], GainClass(0)));
    assert!(ports.gain_calls.is_empty());
}

#[test]
fn fixed_variants_take_the_request_triple_or_the_warm_table() {
    let mut ports = sources();
    ports.class_gains.push((GainClass(5), [9, 9, 9]));
    let fixed = GainRequest { variant: 8, fixed: [4, 5, 6], ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &fixed), ([4, 5, 6], GainClass(11)));
    let incomplete = GainRequest { variant: 9, fixed: [4, 0, 6], ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &incomplete), ([9, 9, 9], GainClass(12)));
}

#[test]
fn the_custom_variant_asks_its_source() {
    let mut ports = sources();
    ports.custom_gains = [7, 8, 9];
    let request = GainRequest { variant: 10, custom_source: 2, ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &request), ([7, 8, 9], GainClass(61)));
}

#[test]
fn the_force_flag_replaces_every_class() {
    let mut ports = sources();
    ports.set_byte(0xa8, 1 << 2);
    ports.forced = GainClass(33);
    let request = GainRequest { variant: 3, ..GainRequest::default() };
    assert_eq!(gain_class(&mut ports, &request), GainClass(33));
}

#[test]
fn the_requested_force_applies_only_to_forcing_requests() {
    let mut ports = sources();
    ports.set_byte(0xa8, 1 << 6);
    ports.forced = GainClass(33);
    let plain = GainRequest { variant: 3, ..GainRequest::default() };
    assert_eq!(gains(&mut ports, &plain).1, GainClass(7));
    let forcing = GainRequest { forced: true, ..plain };
    assert_eq!(gains(&mut ports, &forcing).1, GainClass(33));
}

#[test]
fn mode_25_reads_its_own_flags() {
    let mut ports = sources();
    let request = GainRequest { mode: 25, ..GainRequest::default() };
    assert_eq!(gain_class(&mut ports, &request), GainClass(23));
    ports.set_byte(0x794e, 1);
    assert_eq!(gain_class(&mut ports, &request), GainClass(0));
}

#[test]
fn a_blend_weight_reads_the_output_its_class_column_names() {
    let mut ports = sources();
    ports.set_byte(0x821e, 3);
    ports.set_byte(0x826b, 77);
    assert_eq!(blend_weight(&ports, 10, 0, 1, 0, 0), 77);
}

#[test]
fn a_blend_weight_out_of_range_reads_ten() {
    let mut ports = sources();
    ports.set_byte(0x821e, 16);
    assert_eq!(blend_weight(&ports, 10, 0, 1, 0, 0), 10);
    assert_eq!(blend_weight(&ports, 0x99, 0, 0, 0, 0), 10);
}
