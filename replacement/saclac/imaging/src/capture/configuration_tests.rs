use std::vec::Vec;

use super::configuration::*;
use super::fake::FakeCapture;
use super::ports::{Adjustments, PixelGeometry, Reference};
use super::statistics::*;
use super::{AdjustmentKey, ProfileHandle, ProfileKind};

#[derive(Clone, Copy)]
struct Buffer {
    areas: [Plane; CHANNELS],
    black: [u32; CHANNELS],
}

const EMPTY: Buffer = Buffer { areas: [[0; CELLS]; CHANNELS], black: [0; CHANNELS] };

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Capture(RawStore),
    Keep(RawStore),
    Publish { client: ClientWord, store: Option<RawStore>, first: bool, previous: bool },
}

const fn index(store: RawStore) -> usize {
    match store {
        RawStore::First => 0,
        RawStore::Second => 1,
    }
}

struct Fake {
    geometry: FakeCapture,
    completed: Option<(CaptureParameters, CaptureBlock)>,
    source: Buffer,
    current: [Buffer; 2],
    previous: [Buffer; 2],
    events: Vec<Event>,
    levels: Option<LevelRecord>,
    statistics: Option<StatisticsRecord>,
    updated: Option<CaptureParameters>,
    output_levels: [i32; CHANNELS],
    signed_levels: [i32; CHANNELS],
    selection_status: u32,
    level_index: u32,
    calibrated: i16,
    linearized: u32,
}

impl Fake {
    fn new(store: Option<RawStore>) -> Self {
        let parameters = CaptureParameters { extent: Area { width: 16, height: 16 }, store, profile: ProfileHandle(2), ..CaptureParameters::default() };
        let block = CaptureBlock { window: Area { width: 16, height: 16 }, grid: Area { width: 8, height: 8 } };
        Self {
            geometry: FakeCapture::new(ProfileKind::Tabled, Reference::default()),
            completed: Some((parameters, block)),
            source: Buffer { areas: [[2000; CELLS], [3000; CELLS], [4000; CELLS], [5000; CELLS]], black: [400; CHANNELS] },
            current: [EMPTY; 2],
            previous: [EMPTY; 2],
            events: Vec::new(),
            levels: None,
            statistics: None,
            updated: None,
            output_levels: [0; CHANNELS],
            signed_levels: [0; CHANNELS],
            selection_status: 0,
            level_index: 0,
            calibrated: 99,
            linearized: 100,
        }
    }
}

impl Adjustments for Fake {
    fn byte(&self, key: AdjustmentKey) -> u8 { self.geometry.byte(key) }
}

impl PixelGeometry for Fake {
    fn shared_divisors(&mut self) -> [u32; 2] { self.geometry.shared_divisors() }

    fn scale_divisors(&mut self, profile: ProfileHandle) -> [u32; 2] { self.geometry.scale_divisors(profile) }

    fn channel_pixels(&mut self, pixels: u32) -> ChannelPixels { self.geometry.channel_pixels(pixels) }

    fn aspect(&mut self, profile: ProfileHandle) -> i32 { self.geometry.aspect(profile) }
}

impl ConfigurationPorts for Fake {
    fn completed(&mut self) -> Option<(CaptureParameters, CaptureBlock)> { self.completed }

    fn capture(&mut self, store: RawStore) {
        self.events.push(Event::Capture(store));
        self.current[index(store)] = self.source;
    }

    fn keep(&mut self, store: RawStore) {
        self.events.push(Event::Keep(store));
        self.previous[index(store)] = self.current[index(store)];
    }

    fn publish(&mut self, publication: &Publication) {
        let Publication { client, store, first, previous, .. } = *publication;
        self.events.push(Event::Publish { client, store, first, previous });
    }

    fn raw_pair(&mut self, store: RawStore) -> RawPair<'_> {
        let (current, previous) = (&mut self.current[index(store)], &self.previous[index(store)]);
        let [first, second, third, fourth] = &mut current.areas;
        RawPair { current: [first, second, third, fourth], current_black: &mut current.black, previous: previous.areas.each_ref(), previous_black: &previous.black }
    }

    fn raw(&mut self, store: RawStore) -> RawStatistics<'_> {
        let buffer = &self.current[index(store)];
        RawStatistics { planes: buffer.areas.each_ref(), regions: [[0; 3]; REGIONS], black: buffer.black }
    }

    fn record_levels(&mut self, record: &LevelRecord) { self.levels = Some(*record) }

    fn record_statistics(&mut self, record: &StatisticsRecord) { self.statistics = Some(*record) }

    fn update(&mut self, parameters: &CaptureParameters) { self.updated = Some(*parameters) }

    fn output_levels(&mut self, _: i16, _: ProfileHandle) -> [i32; CHANNELS] { self.output_levels }

    fn signed_levels(&mut self, _: i16, _: ProfileHandle) -> [i32; CHANNELS] { self.signed_levels }

    fn selection_status(&mut self, _: i16, _: ProfileHandle) -> u32 { self.selection_status }

    fn level_index(&mut self, _: i16) -> u32 { self.level_index }

    fn calibrated_level(&mut self, _: u32) -> i16 { self.calibrated }

    fn linearized(&mut self, _: i16) -> u32 { self.linearized }
}

struct Output {
    planes: [Plane; CHANNELS],
    cleared: [u16; 8],
    totals: [u32; CHANNELS],
}

impl Output {
    fn new() -> Self { Self { planes: [[1; CELLS]; CHANNELS], cleared: [7; 8], totals: [9; CHANNELS] } }

    fn buffer(&mut self) -> GridBuffer<'_> { GridBuffer { planes: &mut self.planes, cleared: &mut self.cleared, totals: &mut self.totals } }

    fn firsts(&self) -> [u32; CHANNELS] { self.planes.map(|plane| plane[0]) }
}

const CLIENT: ClientWord = ClientWord(0x1234);

fn configure(configuration: &mut CaptureConfiguration, fake: &mut Fake, selection: Selection, client: Option<ClientWord>) -> (bool, Output) {
    let mut output = Output::new();
    let configured = configuration.configure(fake, selection, client, Some(output.buffer()));
    (configured, output)
}

#[test]
fn another_kind_of_capture_configures_nothing() {
    let mut fake = Fake::new(Some(RawStore::First));
    fake.completed = None;
    let (configured, output) = configure(&mut CaptureConfiguration::default(), &mut fake, Selection::default(), Some(CLIENT));
    assert!(!configured);
    assert!(fake.events.is_empty());
    assert_eq!(output.firsts(), [1; CHANNELS]);
}

#[test]
fn the_first_store_is_captured_kept_and_published() {
    let mut fake = Fake::new(Some(RawStore::First));
    let mut configuration = CaptureConfiguration::default();
    let (configured, output) = configure(&mut configuration, &mut fake, Selection::default(), Some(CLIENT));
    assert!(configured);
    assert_eq!(fake.events, [
        Event::Capture(RawStore::First),
        Event::Keep(RawStore::First),
        Event::Publish { client: CLIENT, store: Some(RawStore::First), first: true, previous: false },
    ]);
    assert!(configuration.first_published());
    assert_eq!(output.firsts(), [2000, 3000, 4000, 5000]);
    assert_eq!((output.cleared, output.totals), ([0; 8], [400; CHANNELS]));
    assert_eq!(fake.updated.map(|parameters| parameters.extent), Some(Area { width: 64, height: 4 }));
}

#[test]
fn a_first_store_capture_without_a_client_is_not_published() {
    let mut fake = Fake::new(Some(RawStore::First));
    let mut configuration = CaptureConfiguration::default();
    configure(&mut configuration, &mut fake, Selection::default(), Some(CLIENT));
    configure(&mut configuration, &mut fake, Selection::default(), None);
    assert!(!configuration.first_published());

    let mut fake = Fake::new(Some(RawStore::Second));
    configure(&mut configuration, &mut fake, Selection::default(), Some(CLIENT));
    assert!(!configuration.first_published());
}

#[test]
fn difference_takes_this_capture_from_the_previous_one() {
    let mut fake = Fake::new(Some(RawStore::First));
    fake.previous[0] = Buffer { areas: [[2500; CELLS], [1000; CELLS], [4000; CELLS], [9000; CELLS]], black: [300; CHANNELS] };
    let selection = Selection { difference: true, level_correction: false };
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, selection, Some(CLIENT));
    assert_eq!(fake.events, [
        Event::Capture(RawStore::First),
        Event::Publish { client: CLIENT, store: Some(RawStore::First), first: true, previous: true },
    ]);
    assert_eq!(output.firsts(), [500, 0, 0, 4000]);
    assert_eq!(output.totals, [0; CHANNELS]);
}

#[test]
fn level_correction_removes_the_estimated_black_level() {
    let mut fake = Fake::new(Some(RawStore::First));
    fake.output_levels = [10; CHANNELS];
    fake.signed_levels = [5, 6, 7, 8];
    fake.selection_status = 1;
    let selection = Selection { difference: false, level_correction: true };
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, selection, Some(CLIENT));
    // Each black sum less 10 per quarter of the 8 x 8 grid: (4 * 240) / 64 = 15, then 64 window
    // pixels per channel times 15 plus the signed level.
    assert_eq!(output.firsts(), [2000 - 64 * 20, 3000 - 64 * 21, 4000 - 64 * 22, 5000 - 64 * 23]);
    assert_eq!(output.totals, [0; CHANNELS]);
    assert_eq!(fake.levels, Some(LevelRecord { output: [10; CHANNELS], signed: [5, 6, 7, 8] }));
    assert_eq!(fake.statistics, Some(StatisticsRecord { grid: Area { width: 8, height: 8 }, black: [400; CHANNELS] }));
    assert_eq!(fake.updated.map(|parameters| parameters.grid), Some(Area { width: 8, height: 8 }));

    let mut fake = Fake::new(Some(RawStore::First));
    fake.signed_levels = [5; CHANNELS];
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, selection, Some(CLIENT));
    assert_eq!(output.firsts()[0], 2000 - 64 * 5);
}

#[test]
fn the_profile_level_selects_a_gain_and_levels_past_the_third_have_none() {
    let mut fake = Fake::new(Some(RawStore::First));
    fake.geometry.set_halfword(0x80f6, 0x100);
    fake.level_index = 1;
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, Selection::default(), Some(CLIENT));
    assert_eq!(output.firsts(), [4000, 6000, 8000, 10_000]);

    fake.level_index = 3;
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, Selection::default(), Some(CLIENT));
    assert_eq!(output.firsts(), [2000, 3000, 4000, 5000]);

    fake.geometry.set_halfword(0x80fa, 0x40);
    fake.calibrated = 0;
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, Selection::default(), Some(CLIENT));
    assert_eq!(output.firsts(), [1000, 1500, 2000, 2500]);
}

#[test]
fn the_linearised_exposure_scales_the_grid() {
    let mut fake = Fake::new(Some(RawStore::Second));
    fake.linearized = 150;
    let (_, output) = configure(&mut CaptureConfiguration::default(), &mut fake, Selection::default(), None);
    assert_eq!(output.firsts(), [3000, 4500, 6000, 7500]);
}

#[test]
fn without_a_store_the_second_buffers_are_kept_and_the_output_is_untouched() {
    let mut fake = Fake::new(None);
    let (configured, output) = configure(&mut CaptureConfiguration::default(), &mut fake, Selection::default(), Some(CLIENT));
    assert!(configured);
    assert_eq!(fake.events, [Event::Keep(RawStore::Second), Event::Publish { client: CLIENT, store: None, first: false, previous: false }]);
    assert_eq!(output.firsts(), [1; CHANNELS]);
    assert!(fake.updated.is_none());
}
