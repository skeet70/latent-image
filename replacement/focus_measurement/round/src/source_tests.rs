use crate::fakes::{Outbound, Ports, Published};
use crate::jobs::Effect;
use crate::pipeline::{PipelineSnapshot, PipelineWord, SnapshotStatus, TimingWord};
use crate::profile::{ProfileSelection, RetainedWord};
use crate::record::{AdjustmentWords, Request, TraceMark};
use crate::source::{ProcessingProfile, SegmentSettings, Source, SourceProfile, SourceRequest, SourceSettings, SourceStrength, Summary};
use xm1_saclac_interface::{stock, Completed, Completion, RecordHandle};
use std::vec::Vec;

fn record() -> RecordHandle { stock::record_handle_from_a2(0x16c0000) }

/// Segment by segment: 28, 22, 9 and 4 values.
fn summary(segments: [i16; 4]) -> Summary {
    let mut values = [0u16; 63];
    for (index, value) in values.iter_mut().enumerate() {
        let segment = match index { 0..28 => 0, 28..50 => 1, 50..59 => 2, _ => 3 };
        *value = segments[segment] as u16;
    }
    Summary(values)
}

fn segment_values(summary: &Summary) -> [i16; 4] { [0, 28, 50, 59].map(|first| summary.0[first] as i16) }

fn measuring() -> SourceProfile {
    SourceProfile {
        measures_source: true,
        keeps_summary: false,
        processing: ProcessingProfile { mode_position: RetainedWord(4), selection: ProfileSelection::Record, ..ProcessingProfile::default() },
        timing: TimingWord(0x77),
        level: 300,
    }
}

const SEGMENTS: [SegmentSettings; 4] = [
    SegmentSettings { ring_length: 2, newest_weight: 1, gain: 64 },
    SegmentSettings { ring_length: 1, newest_weight: 0, gain: 128 },
    SegmentSettings { ring_length: 0, newest_weight: 2, gain: 0 },
    SegmentSettings { ring_length: 3, newest_weight: 0, gain: 128 },
];

fn ports() -> Ports {
    let mut ports = Ports::new(None, ());
    ports.source.profile = measuring();
    ports.source.settings = SourceSettings { segments: SEGMENTS, strengths: [10, 20, 200, 40, 50, 60], carries_extension: false };
    ports.adjustment = AdjustmentWords { controllers: RetainedWord(1), ..AdjustmentWords::default() };
    ports
}

fn completed(strength: u8) -> [u8; 6] { [strength, 9, 9, 9, 9, 9] }

fn collect(source: &mut Source, ports: &mut Ports, parameters: &[u8]) {
    source.effect(Effect::Collect, record(), Some(Completed { completion: Completion::Calculation9a, parameters: stock::parameters(parameters) }), ports);
}

fn requests(ports: &mut Ports) -> Vec<SourceRequest> {
    ports.take().into_iter().filter_map(|traffic| match traffic {
        Outbound::Request(Request::Source(request)) => Some(request),
        _ => None,
    }).collect()
}

#[test]
fn a_profile_that_does_not_measure_the_source_withdraws_it() {
    let mut ports = ports();
    let mut source = Source::new();
    ports.source.profile.processing.selection = ProfileSelection::Unselected;
    source.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!((ports.source.published, ports.source.callbacks_enabled), (Some(Published::Withdrawn), Some(false)));

    ports.source.profile = SourceProfile { measures_source: false, ..measuring() };
    ports.source.callbacks_enabled = None;
    source.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!((ports.source.published, ports.source.callbacks_enabled), (Some(Published::Withdrawn), Some(false)));
    assert!(ports.source.requests.is_empty());
}

#[test]
fn a_fresh_start_summarises_at_the_profiles_timing_with_the_strength_of_its_mode() {
    let mut ports = ports();
    ports.source.summaries = [summary([100, 100, 100, -20])].into();
    let mut source = Source::new();
    source.effect(Effect::Initialize, record(), None, &mut ports);
    let request = ports.source.requests[0];
    assert_eq!((request.timing, request.level, request.strength), (TimingWord(0x77), 300, SourceStrength(128)));
    assert_eq!(ports.source.published, Some(Published::Fresh { summary: summary([100, 100, 100, -20]), strength: SourceStrength(128) }));
    assert_eq!(ports.source.callbacks_enabled, Some(true));

    ports.source.profile.processing.mode_position = RetainedWord(7);
    source.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(ports.source.requests[1].strength, SourceStrength(10));
    ports.source.profile.processing.mode_position = RetainedWord(14);
    source.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(ports.source.requests[2].strength, SourceStrength(60));
}

#[test]
fn readings_are_due_every_third_tick_while_the_controllers_adjustment_is_set() {
    let mut ports = ports();
    let mut source = Source::new();
    source.effect(Effect::Enable, record(), None, &mut ports);
    let ticks: Vec<bool> = (0..5).map(|_| source.due(&mut ports)).collect();
    assert_eq!(ticks, [true, false, false, true, false]);
    source.effect(Effect::Enable, record(), None, &mut ports);
    assert!(source.due(&mut ports));

    ports.adjustment.controllers = RetainedWord(0);
    source.effect(Effect::Enable, record(), None, &mut ports);
    assert!((0..4).all(|_| !source.due(&mut ports)));
}

#[test]
fn a_reading_request_carries_the_primary_sample_and_the_profile() {
    let mut ports = ports();
    ports.snapshot = PipelineSnapshot {
        status: SnapshotStatus(5),
        timing: TimingWord(0x1234),
        levels: [5, 600],
        size: [PipelineWord(64), PipelineWord(48)],
        exposure: [PipelineWord(1), PipelineWord(0x99)],
        ..PipelineSnapshot::default()
    };
    ports.source.sample.level_offset = 1000;
    ports.source.sample.extension = RetainedWord(0x55);
    ports.relative_exposure = -30;
    ports.traces = true;
    let mut source = Source::new();
    source.effect(Effect::Submit, record(), None, &mut ports);
    let traffic = ports.take();
    assert_eq!(traffic[0], Outbound::Trace(TraceMark::SourceRequest));
    let Outbound::Request(Request::Source(request)) = traffic[1] else { panic!("no source request: {traffic:?}") };
    assert_eq!((request.level, request.extension, request.snapshot, request.selection), (970, RetainedWord(0), ports.snapshot, ProfileSelection::Unselected));

    ports.source.settings.carries_extension = true;
    source.effect(Effect::Submit, record(), None, &mut ports);
    assert_eq!(requests(&mut ports)[0].extension, RetainedWord(0x55));
}

#[test]
fn a_reading_averages_each_segment_over_its_ring_and_steps_the_summary_towards_the_mean() {
    let mut ports = ports();
    ports.source.summaries = [summary([100, 100, 100, -20]), summary([104, 90, 77, -25])].into();
    ports.snapshot = PipelineSnapshot { timing: TimingWord(0x4321), levels: [1, 640], ..PipelineSnapshot::default() };
    let mut source = Source::new();
    source.effect(Effect::Initialize, record(), None, &mut ports);
    source.effect(Effect::Submit, record(), None, &mut ports);
    collect(&mut source, &mut ports, &completed(0x2a));

    let request = ports.source.requests[1];
    assert_eq!((request.timing, request.level, request.strength), (TimingWord(0x4321), 640, SourceStrength(0x2a)));
    let Some(Published::Averaged { summary, means, strength }) = ports.source.published else { panic!("not averaged: {:?}", ports.source.published) };
    assert_eq!(segment_values(&means), [103, 90, 77, -22]);
    assert_eq!(segment_values(&summary), [102, 90, 100, -22]);
    assert_eq!(strength, SourceStrength(0x2a));
    assert!(source.changed());
}

#[test]
fn a_segment_with_an_empty_ring_and_no_weight_withholds_the_publication() {
    let mut ports = ports();
    ports.source.settings.segments[2] = SegmentSettings { ring_length: 0, newest_weight: 0, gain: 128 };
    ports.source.summaries = [summary([100, 100, 100, -20]), summary([104, 90, 77, -25])].into();
    let mut source = Source::new();
    source.effect(Effect::Initialize, record(), None, &mut ports);
    source.effect(Effect::Submit, record(), None, &mut ports);
    collect(&mut source, &mut ports, &completed(0x2a));
    assert_eq!(ports.source.published, Some(Published::Fresh { summary: summary([100, 100, 100, -20]), strength: SourceStrength(128) }));
    assert!(!source.changed());
}

#[test]
fn a_release_withdraws_and_a_kept_restart_republishes_with_the_released_strength() {
    let mut ports = ports();
    ports.source.summaries = [summary([100, 100, 100, -20])].into();
    let mut source = Source::new();
    source.effect(Effect::Initialize, record(), None, &mut ports);
    source.effect(Effect::Release, record(), None, &mut ports);
    assert_eq!((ports.source.published, ports.source.callbacks_enabled), (Some(Published::Withdrawn), Some(false)));

    ports.source.profile.keeps_summary = true;
    source.effect(Effect::Initialize, record(), None, &mut ports);
    assert_eq!(ports.source.published, Some(Published::Averaged {
        summary: summary([100, 100, 100, -20]), means: Summary::ZERO, strength: SourceStrength(128),
    }));
    assert_eq!((ports.source.callbacks_enabled, ports.source.requests.len()), (Some(true), 1));
}
