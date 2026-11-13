#![forbid(unsafe_code)]

//! The source measurement: a processing summary of 63 values in four segments, kept for
//! each segment over a ring of recent summaries, smoothed towards the ring's weighted mean
//! after each calc reading and published with that mean in the shared record.

use core::ops::Range;
use crate::jobs;
use crate::pipeline::{PipelineSnapshot, Statistics, TimingWord};
use crate::profile::{ProfileSelection, RetainedHalfword, RetainedWord};
use crate::record::{Ports as RecordPorts, Request, TraceMark};
use xm1_saclac_interface::{Completed, RecordHandle};

pub const SUMMARY: usize = 63;
/// The ring's capacity; a longer configured length is cut to it.
pub const RING: usize = 10;

/// The segments of a summary; values of the last are signed.
const SEGMENTS: [Range<usize>; 4] = [0..28, 28..50, 50..59, 59..63];
const SIGNED_SEGMENT: usize = 3;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Summary(pub [u16; SUMMARY]);

impl Summary {
    pub const ZERO: Self = Self([0; SUMMARY]);
}

/// The strength the processing context weighs the summary with: a configured value when a
/// summary starts afresh, then what each calc reading returns.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceStrength(pub u8);

/// Profile values the processing request carries without the round interpreting them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ProcessingProfile {
    /// The adjustment's mode position for the profile; it also selects the configured strength.
    pub mode_position: RetainedWord,
    pub selection: ProfileSelection,
    /// The capture window scaled for the profile.
    pub window: [RetainedWord; 2],
    pub words: [RetainedWord; 11],
    pub halfword: RetainedHalfword,
    pub byte: u8,
}

/// What ProfileGeometry's processing context summarises.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessingRequest {
    pub profile: ProcessingProfile,
    pub timing: TimingWord,
    pub level: u16,
    pub strength: SourceStrength,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceProfile {
    /// The selected profile measures the source.
    pub measures_source: bool,
    /// The controllers keep the published summary across a restart.
    pub keeps_summary: bool,
    pub processing: ProcessingProfile,
    /// The profile's timing and level a fresh summary is taken at.
    pub timing: TimingWord,
    pub level: u16,
}

/// Profile values the calc request carries without the round interpreting them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SampleProfile {
    pub words: [RetainedWord; 2],
    /// The profile's converted decade and a profile halfword.
    pub halfwords: [RetainedHalfword; 2],
    /// Added to the sample's relative exposure to give the request's level.
    pub level_offset: u16,
    /// Carried only where the configuration asks for it.
    pub extension: RetainedWord,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SegmentSettings {
    pub ring_length: u8,
    /// Weight of the newest summary against the ring's sum.
    pub newest_weight: u8,
    /// Of 128, the step of the published summary towards the ring mean.
    pub gain: u8,
}

/// Configuration bytes of the source measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceSettings {
    pub segments: [SegmentSettings; 4],
    /// The default strength, then those of mode positions 1, 4, 5, 11 and 14.
    pub strengths: [u8; 6],
    /// The calc request carries the profile's extension word.
    pub carries_extension: bool,
}

impl SourceSettings {
    fn strength(&self, mode_position: RetainedWord) -> SourceStrength {
        let index = match mode_position.0 { 1 => 1, 4 => 2, 5 => 3, 11 => 4, 14 => 5, _ => 0 };
        SourceStrength(self.strengths[index].min(128))
    }
}

/// Calc 0x3c11: one reading over the primary statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SourceRequest {
    pub selection: ProfileSelection,
    pub profile: SampleProfile,
    pub snapshot: PipelineSnapshot,
    /// The profile's level offset plus the sample's relative exposure.
    pub level: u16,
    /// Zero unless the configuration carries the profile's extension.
    pub extension: RetainedWord,
}

/// What the shared record shows its readers; they take the summary and the strength.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourcePublication<'a> {
    Withdrawn,
    /// A fresh summary, which stands for its own means.
    Fresh { summary: &'a Summary, strength: SourceStrength },
    Averaged { summary: &'a Summary, means: &'a Summary, strength: SourceStrength },
}

/// The shared helpers and records the source measurement uses.
pub trait Ports: RecordPorts {
    fn source_profile(&mut self, record: RecordHandle) -> SourceProfile;
    fn sample_profile(&mut self, record: RecordHandle) -> SampleProfile;
    fn source_settings(&mut self) -> SourceSettings;
    /// ProfileGeometry's processing context's summary for `request`.
    fn processing_summary(&mut self, request: &ProcessingRequest) -> Summary;
    /// Enables or disables ProfileGeometry's processing callbacks.
    fn enable_processing_callbacks(&mut self, enabled: bool);
    /// The strength the shared record currently shows.
    fn published_strength(&mut self) -> SourceStrength;
    fn publish_source(&mut self, record: RecordHandle, publication: SourcePublication<'_>);
}

pub struct Source {
    tick: u8,
    summary: Summary,
    means: Summary,
    rows: [Summary; RING],
    lengths: [u8; 4],
    positions: [u8; 4],
    strength: SourceStrength,
    submitted_timing: TimingWord,
    submitted_level: u16,
    changed: bool,
}

impl Default for Source { fn default() -> Self { Self::new() } }

impl Source {
    pub const fn new() -> Self {
        Self {
            tick: 0,
            summary: Summary::ZERO,
            means: Summary::ZERO,
            rows: [Summary::ZERO; RING],
            lengths: [0; 4],
            positions: [0; 4],
            strength: SourceStrength(0),
            submitted_timing: TimingWord(0),
            submitted_level: 0,
            changed: false,
        }
    }

    /// Whether the latest collection published.
    pub const fn changed(&self) -> bool { self.changed }

    /// Whether this measurement tick takes a reading: every third tick from the first after
    /// enabling, while the controllers' adjustment is set.
    pub fn due(&mut self, ports: &mut impl Ports) -> bool {
        if ports.adjustment_words().controllers == RetainedWord(0) { return false; }
        self.tick = if self.tick >= 3 { 1 } else { self.tick + 1 };
        self.tick == 1
    }

    /// `completed` is the completion a `Collect` follows.
    pub fn effect(&mut self, effect: jobs::Effect, record: RecordHandle, completed: Option<Completed<'_>>, ports: &mut impl Ports) {
        match effect {
            jobs::Effect::Initialize => self.initialize(record, ports),
            jobs::Effect::Enable => self.tick = 0,
            jobs::Effect::Release => self.release(record, ports),
            jobs::Effect::Submit => self.submit(record, ports),
            jobs::Effect::Collect => {
                let strength = completed.map_or(self.strength, |completed| crate::stock::source_strength(completed.parameters));
                self.changed = self.collect(strength, record, ports);
            }
        }
    }

    fn initialize(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let profile = ports.source_profile(record);
        if profile.processing.selection == ProfileSelection::Unselected || !profile.measures_source {
            ports.publish_source(record, SourcePublication::Withdrawn);
            ports.enable_processing_callbacks(false);
            return;
        }
        if profile.keeps_summary {
            ports.publish_source(record, SourcePublication::Averaged { summary: &self.summary, means: &self.means, strength: self.strength });
            ports.enable_processing_callbacks(true);
            return;
        }
        let settings = ports.source_settings();
        let strength = settings.strength(profile.processing.mode_position);
        self.summary = ports.processing_summary(&ProcessingRequest { profile: profile.processing, timing: profile.timing, level: profile.level, strength });
        self.positions = [0; 4];
        self.rows = [Summary::ZERO; RING];
        for (segment, range) in SEGMENTS.iter().enumerate() {
            let length = settings.segments[segment].ring_length.min(RING as u8);
            self.lengths[segment] = length;
            for row in &mut self.rows[..usize::from(length)] {
                row.0[range.clone()].copy_from_slice(&self.summary.0[range.clone()]);
            }
        }
        ports.publish_source(record, SourcePublication::Fresh { summary: &self.summary, strength });
        ports.enable_processing_callbacks(true);
    }

    fn release(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        self.strength = ports.published_strength();
        ports.publish_source(record, SourcePublication::Withdrawn);
        ports.enable_processing_callbacks(false);
    }

    fn submit(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        let selection = ports.profile(record).selection;
        let profile = ports.sample_profile(record);
        let snapshot = ports.snapshot(Statistics::Primary);
        let level = profile.level_offset.wrapping_add(ports.relative_exposure(snapshot.exposure[1]) as u16);
        let extension = if ports.source_settings().carries_extension { profile.extension } else { RetainedWord(0) };
        self.submitted_timing = snapshot.timing;
        self.submitted_level = snapshot.levels[1];
        if ports.traces_measurement() { ports.trace(TraceMark::SourceRequest); }
        ports.request(Request::Source(SourceRequest { selection, profile, snapshot, level, extension }));
    }

    /// Returns whether every segment averaged, which publishes.
    fn collect(&mut self, strength: SourceStrength, record: RecordHandle, ports: &mut impl Ports) -> bool {
        self.strength = strength;
        let profile = ports.source_profile(record).processing;
        let fresh = ports.processing_summary(&ProcessingRequest { profile, timing: self.submitted_timing, level: self.submitted_level, strength });
        let settings = ports.source_settings();
        let mut averaged = true;
        for (segment, range) in SEGMENTS.iter().enumerate() {
            let position = self.advance(segment);
            self.rows[position].0[range.clone()].copy_from_slice(&fresh.0[range.clone()]);
            averaged &= self.average(segment, range.clone(), position, settings.segments[segment]);
        }
        if averaged {
            ports.publish_source(record, SourcePublication::Averaged { summary: &self.summary, means: &self.means, strength });
            ports.enable_processing_callbacks(true);
        }
        averaged
    }

    fn advance(&mut self, segment: usize) -> usize {
        let next = self.positions[segment].wrapping_add(1);
        self.positions[segment] = if self.lengths[segment] <= next { 0 } else { next };
        usize::from(self.positions[segment])
    }

    /// Sets the segment's means from its ring and steps its summary towards them; false when
    /// the ring is empty and the newest summary has no weight.
    fn average(&mut self, segment: usize, range: Range<usize>, newest: usize, settings: SegmentSettings) -> bool {
        let length = usize::from(self.lengths[segment]);
        let count = length as i32 + i32::from(settings.newest_weight);
        if count == 0 { return false; }
        let value = |value: u16| if segment == SIGNED_SEGMENT { i32::from(value as i16) } else { i32::from(value) };
        for index in range {
            let sum: i32 = self.rows[..length].iter().map(|row| value(row.0[index])).sum();
            let weighted = value(self.rows[newest].0[index]) * i32::from(settings.newest_weight) + sum;
            self.means.0[index] = round_tenths(weighted * 10 / count) as u16;
            let step = (value(self.means.0[index]) - value(self.summary.0[index])) * i32::from(settings.gain) * 10 / 128;
            self.summary.0[index] = self.summary.0[index].wrapping_add(round_tenths(step) as u16);
        }
        true
    }
}

/// `tenths` to the nearest unit, halves away from zero.
fn round_tenths(tenths: i32) -> i32 {
    let tenths = if tenths.unsigned_abs() % 10 > 4 { tenths + if tenths >= 0 { 10 } else { -10 } } else { tenths };
    tenths / 10
}
