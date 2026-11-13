use crate::auxiliary::{Auxiliary, AuxiliaryPublication, AuxiliaryReading, AuxiliaryRequest, AuxiliarySettings, RequestProfile, ResourceIndex};
use crate::fakes::{Outbound, Ports};
use crate::jobs::Effect;
use crate::pipeline::{GainStep, PipelineSnapshot, PipelineWord, SnapshotStatus};
use crate::profile::{ProfileSelection, RetainedHalfword, RetainedWord, StatisticsInUse, WindowRecord, WindowStatus};
use crate::record::{Request, TraceMark};
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};
use std::vec::Vec;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }

fn completed() -> Completed<'static> { Completed { completion: Completion::Calculation8a, parameters: stock::parameters(&[]) } }

const SETTINGS: AuxiliarySettings = AuxiliarySettings {
    ring_length: 4,
    newest_weight: 0,
    gains: [50, 100],
    early_readings: 2,
    follow_band: 4,
    settled_band: 8,
    restore_on_release: false,
};

fn ports() -> Ports {
    let mut ports = Ports::new(None, ());
    ports.auxiliary.settings = SETTINGS;
    ports.auxiliary.default_levels = [100, 200, 400];
    ports
}

fn reading(levels: [u16; 3]) -> AuxiliaryReading { AuxiliaryReading { value: -3, levels, excluded: false } }

fn requests(ports: &mut Ports) -> Vec<AuxiliaryRequest> {
    ports.take().into_iter().filter_map(|traffic| match traffic {
        Outbound::Request(Request::Auxiliary(request)) => Some(request),
        _ => None,
    }).collect()
}

/// An enabled measurement whose next submission is its third pass.
fn on_third_pass(ports: &mut Ports) -> Auxiliary {
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Initialize, record(), None, ports);
    auxiliary.effect(Effect::Enable, record(), None, ports);
    auxiliary.effect(Effect::Submit, record(), None, ports);
    auxiliary.effect(Effect::Submit, record(), None, ports);
    ports.take();
    auxiliary
}

/// Submits the third pass, collects `levels` from it and submits the next first and second passes.
fn follow(auxiliary: &mut Auxiliary, ports: &mut Ports, levels: [u16; 3]) -> bool {
    auxiliary.effect(Effect::Submit, record(), None, ports);
    ports.auxiliary.reading = Some(reading(levels));
    auxiliary.effect(Effect::Collect, record(), Some(completed()), ports);
    let changed = auxiliary.changed();
    auxiliary.effect(Effect::Submit, record(), None, ports);
    auxiliary.effect(Effect::Submit, record(), None, ports);
    ports.take();
    changed
}

/// The curve levels published for the measurement's averages, with no offset.
fn averages(ports: &Ports) -> [u16; 3] { ports.auxiliary.publication.curve }

#[test]
fn a_restart_publishes_the_default_levels_and_a_continued_one_keeps_its_averages() {
    let mut ports = ports();
    ports.auxiliary.lens = ResourceIndex(7);
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(ports.auxiliary.publication, AuxiliaryPublication {
        curve: [100, 200, 400], levels: [100, 200, 400], published: true, curve_word: RetainedWord(7),
    });
    assert_eq!(ports.auxiliary.curves[0].offset, 0);

    ports.auxiliary.default_levels = [1, 1, 1];
    ports.auxiliary.continues = true;
    auxiliary.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(averages(&ports), [100, 200, 400]);
    ports.auxiliary.continues = false;
    auxiliary.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(averages(&ports), [1, 1, 1]);
}

#[test]
fn enabling_keeps_the_auxiliary_statistics_in_use_and_each_first_pass_releases_them() {
    let mut ports = ports();
    ports.records.in_use = StatisticsInUse { primary: true, search: false, auxiliary: false };
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Enable, record(), None, &mut ports);
    assert_eq!(ports.records.in_use, StatisticsInUse { primary: true, search: false, auxiliary: true });
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    assert!(!ports.records.in_use.auxiliary);
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    ports.auxiliary.reading = Some(reading([0; 3]));
    auxiliary.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert!(ports.records.in_use.auxiliary);
}

#[test]
fn passes_cycle_one_to_three_and_only_the_first_reads_the_snapshot() {
    let mut ports = ports();
    ports.traces = true;
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Enable, record(), None, &mut ports);
    ports.take();
    let mut passes = Vec::new();
    for round in 0..4 {
        ports.snapshot.levels = [round, 0];
        auxiliary.effect(Effect::Submit, record(), None, &mut ports);
        let traffic = ports.take();
        assert_eq!(traffic[0], Outbound::Trace(TraceMark::AuxiliaryRequest));
        let [Outbound::Request(Request::Auxiliary(request))] = traffic[1..] else { panic!("one auxiliary request") };
        passes.push((request.pass, request.snapshot.levels[0]));
    }
    assert_eq!(passes, [(1, 0), (2, 0), (3, 0), (1, 3)]);
}

#[test]
fn an_incomplete_snapshot_is_requested_again_on_the_next_pass() {
    let mut ports = ports();
    ports.snapshot.status = SnapshotStatus(1);
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Enable, record(), None, &mut ports);
    for _ in 0..2 {
        auxiliary.effect(Effect::Submit, record(), None, &mut ports);
        let [request] = requests(&mut ports)[..] else { panic!("one request") };
        assert_eq!((request.complete, request.pass), (false, 1));
        assert!(ports.records.in_use.auxiliary);
    }
}

#[test]
fn a_request_carries_the_converted_snapshot_and_the_profile_values() {
    let mut ports = ports();
    ports.records.profile.selection = ProfileSelection::Other(4);
    ports.records.window = WindowRecord { status: WindowStatus(3), first: RetainedWord(1000), halfword: RetainedHalfword(5), second: RetainedWord(6) };
    ports.snapshot = PipelineSnapshot {
        status: SnapshotStatus(7),
        gain_step: GainStep(1),
        levels: [30, 50],
        size: [PipelineWord(4), PipelineWord(5)],
        ..PipelineSnapshot::default()
    };
    ports.gain_offsets[1] = 8;
    ports.auxiliary.rate = 3;
    ports.auxiliary.resource_base = 100;
    ports.auxiliary.request_profile = RequestProfile { resource: ResourceIndex(9), word: RetainedWord(11), halfword: RetainedHalfword(12), ..RequestProfile::default() };
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    let [request] = requests(&mut ports)[..] else { panic!("one request") };
    assert!(request.complete);
    assert_eq!(request.selection, ProfileSelection::Other(4));
    assert_eq!(request.reference_level, 42);
    assert_eq!(request.rate, 3 * 4 * 5 * 4);
    assert_eq!(request.resource_level, 109);
    assert_eq!(request.level_class, RetainedWord(1030));
    assert_eq!(request.window, ports.records.window);
    assert_eq!(request.profile, ports.auxiliary.request_profile);
}

#[test]
fn early_passes_store_and_publish_the_reading_without_following_it() {
    let mut ports = ports();
    ports.auxiliary.curve_offset = 5;
    ports.auxiliary.lens = ResourceIndex(2);
    let mut auxiliary = Auxiliary::new();
    auxiliary.effect(Effect::Initialize, record(), None, &mut ports);
    auxiliary.effect(Effect::Enable, record(), None, &mut ports);
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    ports.auxiliary.reading = Some(reading([900, 900, 900]));
    auxiliary.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert_eq!(ports.auxiliary.value, -3);
    assert_eq!(ports.auxiliary.publication.curve, [107, 207, 407]);
    assert_eq!(ports.auxiliary.publication.levels, [100, 200, 400]);
    assert!(auxiliary.changed());

    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    auxiliary.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert!(!auxiliary.changed());
}

#[test]
fn a_completion_without_a_reading_changes_nothing() {
    let mut ports = ports();
    let mut auxiliary = on_third_pass(&mut ports);
    let before = ports.auxiliary.publication;
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    auxiliary.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert!(!auxiliary.changed());
    assert_eq!(ports.auxiliary.publication, before);
    assert_eq!(ports.auxiliary.grid_refreshes, 0);
}

#[test]
fn following_steps_the_averages_towards_the_ring_means_with_the_early_gain_first() {
    let mut ports = ports();
    let mut auxiliary = on_third_pass(&mut ports);
    assert!(follow(&mut auxiliary, &mut ports, [200, 200, 400]));
    // Ring of four: three readings of the old levels and one new; the early gain moves the whole step, the later one half.
    assert_eq!(ports.auxiliary.publication.levels, [125, 200, 400]);
    assert_eq!(averages(&ports), [125, 200, 400]);
    assert!(follow(&mut auxiliary, &mut ports, [200, 200, 400]));
    assert_eq!((ports.auxiliary.publication.levels, averages(&ports)), ([150, 200, 400], [137, 200, 400]));
    follow(&mut auxiliary, &mut ports, [200, 200, 400]);
    assert_eq!(averages(&ports), [156, 200, 400]);
}

#[test]
fn averages_within_the_follow_band_settle_after_the_early_readings() {
    let mut ports = ports();
    let mut auxiliary = on_third_pass(&mut ports);
    assert!(!follow(&mut auxiliary, &mut ports, [100, 200, 400]));
    assert_eq!(averages(&ports), [100, 200, 400]);
    follow(&mut auxiliary, &mut ports, [100, 200, 400]);
    // Settled: a ring mean outside the follow band but inside the settled band moves nothing.
    follow(&mut auxiliary, &mut ports, [119, 200, 400]);
    assert_eq!(ports.auxiliary.publication.levels, [104, 200, 400]);
    assert_eq!(averages(&ports), [100, 200, 400]);
}

#[test]
fn a_settled_measurement_follows_again_after_its_limit_of_unsettled_readings() {
    let mut ports = ports();
    ports.auxiliary.follow_limit = 2;
    let mut auxiliary = on_third_pass(&mut ports);
    follow(&mut auxiliary, &mut ports, [100, 200, 400]);
    follow(&mut auxiliary, &mut ports, [100, 200, 400]);
    for _ in 0..3 { follow(&mut auxiliary, &mut ports, [400, 200, 400]); }
    assert_eq!(averages(&ports), [100, 200, 400]);
    follow(&mut auxiliary, &mut ports, [400, 200, 400]);
    assert_ne!(averages(&ports), [100, 200, 400]);
}

#[test]
fn excluded_readings_stay_out_of_the_ring() {
    let mut ports = ports();
    let mut auxiliary = on_third_pass(&mut ports);
    auxiliary.effect(Effect::Submit, record(), None, &mut ports);
    ports.auxiliary.reading = Some(AuxiliaryReading { excluded: true, ..reading([900, 900, 900]) });
    auxiliary.effect(Effect::Collect, record(), Some(completed()), &mut ports);
    assert_eq!(ports.auxiliary.publication.levels, [100, 200, 400]);
}

#[test]
fn each_reading_refreshes_the_grid_selection_when_the_profile_asks() {
    let mut ports = ports();
    ports.auxiliary.grid_selection = true;
    let mut auxiliary = on_third_pass(&mut ports);
    follow(&mut auxiliary, &mut ports, [100, 200, 400]);
    assert_eq!(ports.auxiliary.grid_refreshes, 1);
}

#[test]
fn release_restores_the_averages_from_the_published_levels_and_records_the_newest_reading() {
    let mut ports = ports();
    ports.auxiliary.settings.restore_on_release = true;
    ports.auxiliary.curve_offset = 10;
    let mut auxiliary = on_third_pass(&mut ports);
    follow(&mut auxiliary, &mut ports, [200, 200, 400]);
    follow(&mut auxiliary, &mut ports, [200, 200, 400]);
    assert_eq!(ports.auxiliary.publication.curve, [147, 210, 410]);
    auxiliary.effect(Effect::Release, record(), None, &mut ports);
    assert_eq!(ports.auxiliary.capture_levels, Some([200, 200, 400]));
    assert_eq!(ports.auxiliary.publication.curve, [150, 200, 400]);
    ports.auxiliary.continues = true;
    auxiliary.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(averages(&ports), [150, 200, 400]);

    ports.auxiliary.continues = false;
    ports.auxiliary.lens = ResourceIndex(1);
    let mut auxiliary = on_third_pass(&mut ports);
    follow(&mut auxiliary, &mut ports, [200, 200, 400]);
    let published = ports.auxiliary.publication;
    auxiliary.effect(Effect::Release, record(), None, &mut ports);
    assert_eq!(ports.auxiliary.publication.curve, published.curve);
}

#[test]
fn a_record_without_its_grant_leaves_the_shared_record_unchanged() {
    let mut ports = ports();
    ports.records.granted = false;
    let mut auxiliary = on_third_pass(&mut ports);
    follow(&mut auxiliary, &mut ports, [200, 200, 400]);
    assert_eq!(ports.auxiliary.publication, AuxiliaryPublication::default());
    assert_eq!(ports.auxiliary.value, 0);
}
