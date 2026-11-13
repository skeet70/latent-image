#![forbid(unsafe_code)]

//! The auxiliary measurement: three channel levels averaged over a ring of
//! calc1 readings, followed while they leave a settled band, and published
//! through a curve into the shared record.

use crate::jobs;
use crate::pipeline::{PipelineSnapshot, PipelineWord, Statistics, TimingWord};
use crate::profile::{ProfileSelection, RetainedHalfword, RetainedWord, StatisticsInUse, WindowRecord};
use crate::record::{Ports as RecordPorts, Request, TraceMark};
use xm1_saclac_interface::{Completed, RecordHandle};

/// The ring's capacity; a longer configured length is cut to it.
pub const RING: usize = 64;

/// One calc1 reading of the auxiliary statistics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuxiliaryReading {
    /// Stored in the shared record as it arrives.
    pub value: i16,
    pub levels: [u16; 3],
    /// The reading stays out of the ring.
    pub excluded: bool,
}

/// What the auxiliary measurement publishes in the shared record.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuxiliaryPublication {
    pub curve: [u16; 3],
    /// The ring means of the latest followed reading.
    pub levels: [u16; 3],
    pub published: bool,
    pub curve_word: RetainedWord,
}

/// Profile values the curve and the calc1 request carry without the round interpreting them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuxiliaryProfile {
    pub adjustment: RetainedWord,
    pub words: [RetainedWord; 5],
    pub bytes: [u8; 6],
}

/// A resource index the configuration converts to a level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ResourceIndex(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CurveInputs {
    pub profile: AuxiliaryProfile,
    pub lens: ResourceIndex,
    /// Zero when the measurement starts.
    pub offset: i16,
    pub averages: [u16; 3],
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Curve {
    pub levels: [u16; 3],
    pub word: RetainedWord,
}

/// Configuration bytes of the auxiliary measurement.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct AuxiliarySettings {
    pub ring_length: u8,
    /// Weight of the newest reading against the ring's sum.
    pub newest_weight: u8,
    /// Hundredths of the step towards the ring mean, once `early_readings` readings have been followed and before.
    pub gains: [u8; 2],
    pub early_readings: u8,
    /// Half-width about 128 of the band that ends following.
    pub follow_band: u8,
    /// Half-width about 128 of the band the settled averages stay in.
    pub settled_band: u8,
    /// Releasing restores the averages from the published levels.
    pub restore_on_release: bool,
}

/// Values the calc1 request takes from the profile and shared records.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RequestProfile {
    pub profile: AuxiliaryProfile,
    pub resource: ResourceIndex,
    pub word: RetainedWord,
    pub halfword: RetainedHalfword,
}

/// Calc1 0x3d02: one auxiliary measurement pass.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AuxiliaryRequest {
    /// The snapshot is complete.
    pub complete: bool,
    /// Pass 1 to 3; pass 1 builds the request.
    pub pass: u8,
    pub selection: ProfileSelection,
    pub profile: RequestProfile,
    pub window: WindowRecord,
    pub snapshot: PipelineSnapshot,
    /// The snapshot's second level less its gain offset.
    pub reference_level: u16,
    pub resource_level: i16,
    pub level_class: RetainedWord,
    pub rate: u32,
}

/// The shared helpers and records the auxiliary measurement uses.
pub trait Ports: RecordPorts {
    /// The reading a completion carries, taken out of calc1's buffer; `None` when it carries none.
    fn reading(&mut self, completed: Completed<'_>) -> Option<AuxiliaryReading>;
    fn auxiliary_settings(&mut self) -> AuxiliarySettings;
    /// The levels a restarted measurement starts from.
    fn default_levels(&mut self) -> [u16; 3];
    fn curve(&mut self, inputs: &CurveInputs) -> Curve;
    fn lens_resource(&mut self) -> ResourceIndex;
    fn resource_level(&mut self, resource: ResourceIndex) -> i16;
    /// Out-of-band readings a settled measurement takes before it follows again.
    fn follow_limit(&mut self, selection: ProfileSelection, profile: RetainedWord, timing: TimingWord) -> u32;
    /// The configured rate scaled by the statistics image's dimensions.
    fn rate(&mut self, size: [PipelineWord; 2]) -> u32;
    fn level_class(&mut self, level: i16, selection: ProfileSelection, window: RetainedWord) -> RetainedWord;
    fn refresh_grid_selection(&mut self, record: RecordHandle);
    /// Whether the controllers keep the averages across a restart.
    fn auxiliary_continues(&mut self, record: RecordHandle) -> bool;
    fn auxiliary_profile(&mut self, record: RecordHandle) -> AuxiliaryProfile;
    fn request_profile(&mut self, record: RecordHandle) -> RequestProfile;
    fn follow_profile(&mut self, record: RecordHandle) -> RetainedWord;
    /// The shared record's offset the published curve starts from.
    fn curve_offset(&mut self, record: RecordHandle) -> i16;
    fn auxiliary_publication(&mut self, record: RecordHandle) -> AuxiliaryPublication;
    fn publish_auxiliary(&mut self, record: RecordHandle, publication: AuxiliaryPublication);
    fn store_auxiliary_value(&mut self, record: RecordHandle, value: i16);
    /// Whether each reading refreshes the profile's grid selection.
    fn grid_selection(&mut self, record: RecordHandle) -> bool;
    /// Writes `levels` where an attached capture buffer keeps them.
    fn record_capture_levels(&mut self, record: RecordHandle, levels: [u16; 3]);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Ring {
    levels: [[u16; RING]; 3],
    newest: u8,
    length: u8,
}

impl Ring {
    fn newest(&self) -> [u16; 3] { self.levels.map(|channel| channel[usize::from(self.newest)]) }

    fn sums(&self) -> [u32; 3] {
        self.levels.map(|channel| channel[..usize::from(self.length)].iter().map(|&level| u32::from(level)).sum())
    }

    fn append(&mut self, levels: [u16; 3]) {
        let next = self.newest.wrapping_add(1);
        self.newest = if next >= self.length { 0 } else { next };
        let newest = usize::from(self.newest);
        for (channel, level) in self.levels.iter_mut().zip(levels) { channel[newest] = level; }
    }
}

/// The auxiliary lane's state between rounds.
pub struct Auxiliary {
    pass: u8,
    readings: u8,
    unsettled: u8,
    following: bool,
    /// Hundredths.
    averages: [u32; 3],
    ring: Ring,
    means: [u16; 3],
    request: Option<AuxiliaryRequest>,
    changed: bool,
}

impl Default for Auxiliary { fn default() -> Self { Self::new() } }

impl Auxiliary {
    pub const fn new() -> Self {
        Self {
            pass: 0,
            readings: 0,
            unsettled: 0,
            following: false,
            averages: [0; 3],
            ring: Ring { levels: [[0; RING]; 3], newest: 0, length: 0 },
            means: [0; 3],
            request: None,
            changed: false,
        }
    }

    /// Whether the latest collection changed the published curve.
    pub const fn changed(&self) -> bool { self.changed }

    /// `completed` is the completion a `Collect` follows.
    pub fn effect(&mut self, effect: jobs::Effect, record: RecordHandle, completed: Option<Completed<'_>>, ports: &mut impl Ports) {
        match effect {
            jobs::Effect::Initialize => self.initialize(record, ports),
            jobs::Effect::Enable => self.enable(record, ports),
            jobs::Effect::Release => self.release(record, ports),
            jobs::Effect::Submit => self.submit(record, ports),
            jobs::Effect::Collect => {
                self.changed = completed.is_some_and(|completed| self.collect(record, completed, ports));
            }
        }
    }

    fn initialize(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        if !ports.auxiliary_continues(record) {
            let levels = ports.default_levels();
            self.averages = levels.map(|level| u32::from(level) * 100);
            self.means = levels;
            self.following = true;
            self.readings = 0;
        }
        let profile = ports.auxiliary_profile(record);
        let lens = ports.lens_resource();
        let curve = ports.curve(&CurveInputs { profile, lens, offset: 0, averages: self.levels() });
        ports.publish_auxiliary(record, self.publication(curve));
    }

    fn enable(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        self.pass = 0;
        self.ring.newest = 0;
        self.ring.length = ports.auxiliary_settings().ring_length.min(RING as u8);
        let length = usize::from(self.ring.length);
        let levels = self.levels();
        for (channel, level) in self.ring.levels.iter_mut().zip(levels) { channel[..length].fill(level); }
        set_auxiliary_in_use(record, true, ports);
    }

    fn release(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        self.publish(record, ports);
        let published = ports.auxiliary_publication(record);
        if ports.auxiliary_settings().restore_on_release && published.curve_word == RetainedWord(0) {
            self.averages = published.levels.map(|level| u32::from(level) * 100);
            ports.publish_auxiliary(record, AuxiliaryPublication { curve: published.levels, ..published });
        }
        ports.record_capture_levels(record, self.ring.newest());
    }

    fn submit(&mut self, record: RecordHandle, ports: &mut impl Ports) {
        self.pass = if self.pass >= 3 { 1 } else { self.pass + 1 };
        let request = match self.request {
            Some(request) if self.pass != 1 => AuxiliaryRequest { pass: self.pass, ..request },
            _ => {
                let request = build_request(record, ports);
                if request.complete { set_auxiliary_in_use(record, false, ports); } else { self.pass = 0; }
                request
            }
        };
        self.request = Some(request);
        if ports.traces_measurement() { ports.trace(TraceMark::AuxiliaryRequest); }
        ports.request(Request::Auxiliary(request));
    }

    fn collect(&mut self, record: RecordHandle, completed: Completed<'_>, ports: &mut impl Ports) -> bool {
        let Some(reading) = ports.reading(completed) else { return false };
        if self.pass == 3 {
            set_auxiliary_in_use(record, true, ports);
            self.follow(reading, record, ports);
        }
        ports.store_auxiliary_value(record, reading.value);
        let changed = self.publish(record, ports);
        if ports.grid_selection(record) { ports.refresh_grid_selection(record); }
        changed
    }

    fn follow(&mut self, reading: AuxiliaryReading, record: RecordHandle, ports: &mut impl Ports) {
        self.readings = match self.readings.wrapping_add(1) { 255 => 254, count => count };
        if self.request.is_some_and(|request| request.complete) && !reading.excluded { self.ring.append(reading.levels); }
        let settings = ports.auxiliary_settings();
        let weight = u32::from(settings.newest_weight);
        let count = u32::from(self.ring.length) + weight;
        let newest = self.ring.newest();
        let sums = self.ring.sums();
        let means: [u32; 3] = core::array::from_fn(|channel| {
            u32::from(newest[channel]).wrapping_mul(weight).wrapping_add(sums[channel]).checked_div(count).unwrap_or(0)
        });
        let ratios = [
            ratio(self.averages[1].wrapping_mul(means[0]), self.averages[0].wrapping_mul(means[1])),
            ratio(self.averages[2].wrapping_mul(means[0]), self.averages[0].wrapping_mul(means[2])),
        ];
        if !self.following {
            if ratios.iter().all(|&ratio| within(ratio, settings.settled_band)) {
                self.unsettled = 0;
            } else if u32::from(self.unsettled) < self.follow_limit(record, ports) {
                self.unsettled = self.unsettled.wrapping_add(1);
            } else {
                self.following = true;
            }
        } else if ratios.iter().all(|&ratio| within(ratio, settings.follow_band)) {
            if self.readings >= settings.early_readings {
                self.unsettled = 0;
                self.following = false;
            }
        } else {
            let gain = i32::from(if self.readings < settings.early_readings { settings.gains[1] } else { settings.gains[0] });
            for (average, mean) in self.averages.iter_mut().zip(means) {
                let step = (mean.wrapping_mul(100) as i32).wrapping_sub(*average as i32).wrapping_mul(gain) / 100;
                *average = average.wrapping_add(step as u32);
            }
        }
        self.means = means.map(|mean| mean as u16);
    }

    fn follow_limit(&self, record: RecordHandle, ports: &mut impl Ports) -> u32 {
        let selection = ports.profile(record).selection;
        let profile = ports.follow_profile(record);
        let timing = ports.snapshot(Statistics::Auxiliary).timing;
        ports.follow_limit(selection, profile, timing)
    }

    /// Publishes the curve for the current averages; true when its levels changed.
    fn publish(&mut self, record: RecordHandle, ports: &mut impl Ports) -> bool {
        let profile = ports.auxiliary_profile(record);
        let lens = ports.lens_resource();
        let offset = ports.curve_offset(record).wrapping_add(ports.resource_level(lens));
        let curve = ports.curve(&CurveInputs { profile, lens, offset, averages: self.levels() });
        let published = ports.auxiliary_publication(record);
        ports.publish_auxiliary(record, self.publication(curve));
        published.curve != curve.levels
    }

    fn levels(&self) -> [u16; 3] { self.averages.map(|average| (average / 100) as u16) }

    fn publication(&self, curve: Curve) -> AuxiliaryPublication {
        AuxiliaryPublication { curve: curve.levels, levels: self.means, published: true, curve_word: curve.word }
    }
}

/// Of two channel products, 128 when equal.
fn ratio(numerator: u32, denominator: u32) -> u32 {
    (u64::from(numerator) << 7).checked_div(u64::from(denominator)).map_or(0, |ratio| ratio as u32)
}

fn within(ratio: u32, band: u8) -> bool {
    let band = u32::from(band);
    band.wrapping_add(ratio) >= 128 && ratio <= band + 128
}

fn set_auxiliary_in_use(record: RecordHandle, used: bool, ports: &mut impl Ports) {
    let in_use = ports.statistics_in_use();
    ports.set_statistics_in_use(record, StatisticsInUse { auxiliary: used, ..in_use });
}

fn build_request(record: RecordHandle, ports: &mut impl Ports) -> AuxiliaryRequest {
    let selection = ports.profile(record).selection;
    let profile = ports.request_profile(record);
    let window = ports.window();
    let snapshot = ports.snapshot(Statistics::Auxiliary);
    let reference_level = snapshot.levels[1].wrapping_sub(ports.gain_offset(snapshot.gain_step) as u16);
    let rate = ports.rate(snapshot.size).wrapping_mul(4);
    let resource_level = ports.resource_level(profile.resource);
    let level_class = ports.level_class(snapshot.levels[0] as i16, selection, window.first);
    AuxiliaryRequest { complete: snapshot.status.complete(), pass: 1, selection, profile, window, snapshot, reference_level, resource_level, level_class, rate }
}
