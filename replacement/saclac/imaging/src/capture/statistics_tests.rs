use super::fake::FakeCapture;
use super::ports::Reference;
use super::statistics::*;
use super::{ProfileHandle, ProfileKind};

const BLACK_FIRST: u32 = 0x1378;
const PEDESTAL_FIRST: u32 = 0x1380;
const TRANSFER_ROW: u32 = 0x1386;

fn ports() -> FakeCapture {
    let mut ports = FakeCapture::new(ProfileKind::Tabled, Reference::default());
    ports.set_halfword(BLACK_FIRST, 10);
    ports.set_byte(TRANSFER_ROW, 0xff);
    ports
}

fn capture_parameters() -> CaptureParameters {
    let mut regions = [Area::default(); REGION_AREAS];
    regions[0] = Area { width: 16, height: 16 };
    CaptureParameters { extent: Area { width: 16, height: 16 }, grid: Area { width: 10, height: 10 }, regions, profile: ProfileHandle(3), ..CaptureParameters::default() }
}

struct Source {
    planes: [Plane; CHANNELS],
    regions: [[u64; 3]; REGIONS],
    black: [u32; CHANNELS],
}

impl Source {
    fn new() -> Self { Self { planes: [[1000; CELLS], [2000; CELLS], [3000; CELLS], [4000; CELLS]], regions: [[10_000, 20_000, 30_000]; REGIONS], black: [0; CHANNELS] } }

    fn raw(&self) -> RawStatistics<'_> { RawStatistics { planes: [&self.planes[0], &self.planes[1], &self.planes[2], &self.planes[3]], regions: self.regions, black: self.black } }
}

struct Output {
    planes: [Plane; CHANNELS],
    regions: [Region; REGION_AREAS],
}

fn prepared(ports: &mut FakeCapture, source: &Source, parameters: &mut CaptureParameters) -> Output {
    let mut output = Output { planes: [[0; CELLS]; CHANNELS], regions: [[0; CHANNELS]; REGION_AREAS] };
    prepare(ports, &source.raw(), parameters, Prepared { planes: &mut output.planes, regions: &mut output.regions });
    output
}

#[test]
fn black_level_is_removed_per_channel_pixel() {
    let mut parameters = capture_parameters();
    let output = prepared(&mut ports(), &Source::new(), &mut parameters);
    // 256 pixels give each channel 64; 64 * 10 comes off the first channel.
    assert_eq!([output.planes[0][0], output.planes[1][17], output.planes[2][255], output.planes[3][0]], [360, 2000, 3000, 4000]);
    assert_eq!(output.regions[0], [9360, 20_000, 30_000, 20_000]);
}

#[test]
fn regions_are_normalised_to_the_profile_pixels_and_every_area_rescaled() {
    let mut parameters = capture_parameters();
    let output = prepared(&mut ports(), &Source::new(), &mut parameters);
    // An empty area takes the 4-pixel floor (10 off the first channel), then the profile's 8 x 8
    // pixels, 16 per channel, scaled by 64 / 16.
    assert_eq!(output.regions[1], [39_960, 80_000, 120_000, 80_000]);
    assert_eq!(parameters.extent, Area { width: 64, height: 4 });
    assert_eq!(parameters.regions[0], Area { width: 64, height: 4 });
    assert!(parameters.regions[1..].iter().all(|area| *area == Area { width: 64, height: 1 }));
    assert_eq!(parameters.grid, Area { width: 10, height: 10 });
}

#[test]
fn preparation_corrects_persistent_regions_beyond_the_three_produced_regions() {
    let mut ports = ports();
    ports.set_byte(TRANSFER_ROW, 1);
    ports.set_halfword(PEDESTAL_FIRST, 5);
    let mut source = Source::new();
    source.black = [100; CHANNELS];
    let mut parameters = CaptureParameters { reference: 20, ..capture_parameters() };
    parameters.regions[3..].fill(Area { width: 16, height: 16 });
    let mut planes = [[0; CELLS]; CHANNELS];
    let mut regions = [[1000, 2000, 3000, 4000]; REGION_AREAS];
    prepare(&mut ports, &source.raw(), &mut parameters, Prepared { planes: &mut planes, regions: &mut regions });
    assert_eq!(regions[3..], [[1064, 1744, 2744, 3744]; 7]);
    assert_eq!(parameters.regions[3..], [Area { width: 64, height: 4 }; 7]);
    prepare(&mut ports, &source.raw(), &mut parameters, Prepared { planes: &mut planes, regions: &mut regions });
    assert_eq!(regions[3..], [[1128, 1488, 2488, 3488]; 7]);
}

#[test]
fn the_transfer_correction_applies_from_its_threshold_row() {
    let mut ports = ports();
    ports.set_byte(TRANSFER_ROW, 1);
    ports.set_halfword(PEDESTAL_FIRST, 5);
    let mut source = Source::new();
    source.black = [100; CHANNELS];
    let mut parameters = CaptureParameters { reference: 20, ..capture_parameters() };
    let output = prepared(&mut ports, &source, &mut parameters);
    // Black share 400 * 64 / 100 = 256; the first channel's pedestal 64 * (10 + 5).
    assert_eq!([output.planes[0][0], output.planes[1][0]], [360 + 960 - 256, 2000 - 256]);

    ports.aspect = -15;
    let mut parameters = CaptureParameters { reference: 20, ..capture_parameters() };
    let output = prepared(&mut ports, &source, &mut parameters);
    assert_eq!(output.planes[0][0], 360);
}

#[test]
fn samples_are_capped_at_the_channel_limit() {
    let mut source = Source::new();
    source.planes[0][5] = u32::MAX;
    let mut parameters = capture_parameters();
    let output = prepared(&mut ports(), &source, &mut parameters);
    assert_eq!(output.planes[0][5], 64 * 0x3fff);
}

#[test]
fn a_channel_without_pixels_saturates_to_its_limit() {
    let mut ports = ports();
    ports.pixel_shares = [0, 1, 1];
    let mut parameters = capture_parameters();
    let output = prepared(&mut ports, &Source::new(), &mut parameters);
    assert_eq!(output.planes[0][0], 64 * 0x3fff);
}

#[test]
fn the_grid_copy_takes_planes_and_black_sums() {
    let mut source = Source::new();
    source.black = [1, 2, 3, 4];
    let (mut planes, mut cleared, mut totals) = ([[0; CELLS]; CHANNELS], [9u16; 8], [0; CHANNELS]);
    copy_grid(&source.raw(), &mut GridBuffer { planes: &mut planes, cleared: &mut cleared, totals: &mut totals });
    assert_eq!(planes, source.planes);
    assert_eq!(cleared, [0; 8]);
    assert_eq!(totals, [1, 2, 3, 4]);
}
