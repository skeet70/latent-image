use xm1_saclac_interface::calc::SharedAddress;

use crate::adjust::motion::drive::*;
use crate::adjust::motion::fakes::Bench;
use crate::stock::motion as trace;

const KIND: DriveKind = DriveKind(3);

fn bench() -> Bench {
    Bench { first_ramp_speed: 2000, first_ramp_acceleration: 100_000, second_ramp_acceleration: 50_000, slowest_ramped_speed: 100, ..Bench::default() }
}

fn planned(start: i16, end: i16, speed: u32, bench: &mut Bench) -> (DriveProfile, Planning) {
    let mut profile = DriveProfile { start, end, reverse: end < start, ..DriveProfile::default() };
    let planning = profile.plan(speed, KIND, bench);
    (profile, planning)
}

fn total(profile: &DriveProfile) -> u32 { profile.cruise_duration + 2 * (profile.first.duration + profile.second.duration) }

#[test]
fn the_square_root_is_correctly_rounded() {
    let mut bits = 1u32;
    while bits < 0x7f80_0000 {
        let value = f32::from_bits(bits);
        assert_eq!(square_root(value).to_bits(), (f64::from(value).sqrt() as f32).to_bits(), "{value:e}");
        bits += 0x0001_0f31;
    }
    assert_eq!(square_root(4.0), 2.0);
    assert_eq!(square_root(0.0), 0.0);
    assert!(square_root(-1.0).is_nan());
}

#[test]
fn a_short_drive_ramps_up_and_down_without_cruising() {
    let mut bench = bench();
    let (profile, _) = planned(100, 300, 4000, &mut bench);
    assert_eq!(profile.first, Ramp { speed: 2000, acceleration: 100_000, distance: 20, duration: 20_000 });
    assert_eq!(profile.second, Ramp { speed: 3464, acceleration: 50_000, distance: 80, duration: 29_282 });
    assert_eq!((profile.cruise_distance, profile.cruise_duration), (0, 0));
    assert_eq!(bench.kinds, [(KIND, DriveMode::MEASUREMENT); 3]);
    assert_eq!(profile.position_at(0), 100);
    assert_eq!(profile.position_at(20_000), 120);
    assert_eq!(profile.position_at(total(&profile) as i32), 300);
    assert_eq!(profile.position_at(total(&profile) as i32 + 1), 300);
    assert_eq!(profile.time_to(100), 0);
    assert_eq!(profile.time_to(120), 20_000);
    assert_eq!(profile.time_to(300), total(&profile));
}

#[test]
fn a_long_drive_cruises_at_the_requested_speed() {
    let (profile, planning) = planned(0, 1000, 3000, &mut bench());
    assert_eq!(profile.second, Ramp { speed: 3000, acceleration: 50_000, distance: 50, duration: 20_000 });
    assert_eq!((profile.cruise_distance, profile.cruise_duration), (860, 286_666));
    assert_eq!(planning.requested_speed, 3000);
    assert_eq!(profile.position_at(40_000 + 100_000), 370);
    assert_eq!(profile.time_to(370), 140_000);
    assert_eq!(profile.time_to(1000), total(&profile));
}

#[test]
fn positions_and_times_agree_along_the_drive() {
    for (start, end, speed) in [(100, 300, 4000), (0, 1000, 3000), (500, -200, 2500)] {
        let (profile, _) = planned(start, end, speed, &mut bench());
        for time in (0..=total(&profile) as i32).step_by(997) {
            let position = profile.position_at(time);
            assert!((start.min(end)..=start.max(end)).contains(&position));
            let back = profile.position_at(profile.time_to(position) as i32);
            assert!((i32::from(back) - i32::from(position)).abs() <= 1, "{start}->{end} at {time}: {position} came back as {back}");
        }
    }
}

#[test]
fn a_slow_request_is_driven_as_one_cruise() {
    let (profile, planning) = planned(0, 200, 50, &mut bench());
    assert_eq!((profile.first.distance, profile.second.distance), (0, 0));
    assert_eq!((profile.cruise_distance, profile.cruise_duration), (200, 4_000_000));
    assert_eq!(profile.second.speed, 50);
    assert_eq!(planning.first_speed, 50);
    assert_eq!(profile.position_at(1_000_000), 50);
}

#[test]
fn a_reverse_drive_moves_down_from_its_start() {
    let (profile, _) = planned(300, 100, 4000, &mut bench());
    assert_eq!(profile.position_at(20_000), 280);
    assert_eq!(profile.time_to(280), 20_000);
    assert_eq!(profile.position_at(total(&profile) as i32), 100);
}

#[test]
fn without_accelerations_a_drive_has_no_ramps() {
    let mut bench = Bench { first_ramp_speed: 2000, ..Bench::default() };
    let (profile, _) = planned(0, 100, 1000, &mut bench);
    assert_eq!((profile.first.distance, profile.second.distance), (0, 0));
    assert_eq!(profile.cruise_duration, 100_000);
}

#[test]
fn a_planning_traces_the_start_time_then_its_eleven_words() {
    let (_, planning) = planned(100, 300, 4000, &mut bench());
    let section = SharedAddress(0x1000_0000);
    let mut memory = Bench::default();
    planning.trace(-7, section, &mut memory);
    let expected = [-7i32 as u32, 100_000, 50_000, 3464, 2000, 200, 20, 20_000, 80, 29_282, 0, 0];
    let stores: std::vec::Vec<_> = (0..12).map(|word| (trace::drive_planning(section, word).0, expected[word as usize])).collect();
    assert_eq!(memory.stores, stores);
}
