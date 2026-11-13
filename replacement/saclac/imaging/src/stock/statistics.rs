//! The capture statistics' stock side: the imaging pipeline's raw statistics buffers and capture
//! parameters, saclac's statistics stores, the caller's selection and dump, and the profile-layer
//! services the processing configuration calls, at their X-M1 1.50 addresses and layouts.

use core::ptr::{copy_nonoverlapping, read_volatile, write_bytes, write_volatile};

use super::capture::StockCapture;
use crate::capture::configuration::{
    CaptureBlock, CaptureConfiguration, ClientWord, ConfigurationPorts, LevelRecord, Publication, RawPair, Selection, StatisticsRecord,
};
use crate::capture::pipeline::{StockBlock, STATISTICS_BLOCK_BYTES, STATISTICS_PARAMETER_BYTES};
use crate::capture::ports::{Adjustments, PixelGeometry};
use crate::capture::statistics::{
    prepare, Area, CaptureParameters, ChannelPixels, GridBuffer, Plane, Prepared, RawStatistics, RawStore, Region, CHANNELS, REGIONS,
};
use crate::capture::{AdjustmentKey, ProfileHandle};

/// The capture parameters the imaging pipeline records (0x16f8160) and its callers copy.
pub type ParameterBytes = [u8; STATISTICS_PARAMETER_BYTES];
/// The readout selection block 0xf2e7bc builds: mode at +0x1c, value at +0x20; 0xffd3a0 writes
/// the previous-buffer words at +0x24 and +0x28 for 0xedb05c.
pub const SELECTION_BYTES: usize = 0x2c;
pub type SelectionBytes = [u8; SELECTION_BYTES];

/// A raw statistics buffer's length, as 0xffd3a0 copies it.
const RAW_BYTES: usize = 0x29b0;
/// The raw buffer's four plane areas (0xedae88 selectors 3..=5, and 0x1450 after them).
const RAW_PLANES: u32 = 0x850;
const PLANE_BYTES: u32 = 0x400;
/// The plane area each grid channel reads (0xedae88 selectors 3..=6).
const GRID_CHANNELS: [u32; CHANNELS] = [0x850, 0xc50, 0x1050, 0xc50];
/// Three regions of four doublewords (selector 7); the fourth doubleword is not read.
const RAW_REGIONS: u32 = 0x1850;
const RAW_REGION_BYTES: u32 = 0x20;
/// The four black sums (selectors 13..=16).
const RAW_BLACK: u32 = 0x2190;

/// Current and previous buffer of each store (0xffd3f8, 0xffd428, 0xffd460..0xffd478).
const fn store_buffers(store: RawStore) -> [u32; 2] {
    match store {
        RawStore::First => [0x5ffb_9120, 0x5ffb_e480],
        RawStore::Second => [0x5ffb_bad0, 0x5ffc_0e30],
    }
}

/// The selection's previous-buffer words (0xffd518..0xffd534).
const PREVIOUS_WORDS: [u32; 2] = [0x5ffb_e480, 0x5ffc_0e30];
const SELECTION_MODE: usize = 0x1c;
const SELECTION_VALUE: usize = 0x20;
const SELECTION_PREVIOUS: usize = 0x24;
/// The readout mode that differences the first store.
const DIFFERENCE_MODE: u32 = 3;

/// The publication's parameter block, 0x48 bytes from the defaults 0xed8af4 copies.
const PUBLICATION_BYTES: usize = 0x48;

/// Offsets in the caller's dump (0xffd72c..0xffd794, 0xffd8c8..0xffd9b4).
const DUMP_LEVELS: u32 = 0x9_e564;
const DUMP_STATISTICS: u32 = 0x9_ced0;

fn word(bytes: &[u8], offset: usize) -> u32 { u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) }

fn half(bytes: &[u8], offset: usize) -> u16 { u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) }

fn put_word(bytes: &mut [u8], offset: usize, value: u32) { bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes()) }

fn area(bytes: &[u8], offset: usize) -> Area { Area { width: word(bytes, offset), height: word(bytes, offset + 4) } }

fn put_area(bytes: &mut [u8], offset: usize, area: Area) {
    put_word(bytes, offset, area.width);
    put_word(bytes, offset + 4, area.height);
}

/// The parameters as 0xffd04c, 0xffd3a0 and their helpers read them.
pub fn parameters(bytes: &ParameterBytes) -> CaptureParameters {
    CaptureParameters {
        extent: area(bytes, 0),
        grid: area(bytes, 8),
        regions: core::array::from_fn(|index| area(bytes, 0x10 + 8 * index)),
        level: half(bytes, 0x64),
        reference: half(bytes, 0x66) as i16,
        reference_span: half(bytes, 0x68),
        decade: half(bytes, 0x6a) as i16,
        companion: half(bytes, 0x6c),
        setting: word(bytes, 0x70),
        profile: ProfileHandle(word(bytes, 0x74)),
        lens: word(bytes, 0x78),
        lens_reference: word(bytes, 0x7c),
        store: match bytes[0x8c] {
            1 => Some(RawStore::First),
            2 => Some(RawStore::Second),
            _ => None,
        },
        exposure: half(bytes, 0x96) as i16,
    }
}

/// Writes the areas the statistics rescale back to the caller's parameters.
pub fn store_areas(parameters: &CaptureParameters, bytes: &mut ParameterBytes) {
    put_area(bytes, 0, parameters.extent);
    put_area(bytes, 8, parameters.grid);
    for (index, region) in parameters.regions.iter().enumerate() {
        put_area(bytes, 0x10 + 8 * index, *region);
    }
}

fn plane(address: u32) -> &'static Plane { unsafe { &*(address as usize as *const Plane) } }

fn plane_mut(address: u32) -> &'static mut Plane { unsafe { &mut *(address as usize as *mut Plane) } }

fn black_mut(address: u32) -> &'static mut [u32; CHANNELS] { unsafe { &mut *(address as usize as *mut [u32; CHANNELS]) } }

/// A raw statistics buffer in place.
fn raw(buffer: u32) -> RawStatistics<'static> {
    let doubleword = |address: u32| unsafe { read_volatile(address as usize as *const u64) };
    RawStatistics {
        planes: GRID_CHANNELS.map(|offset| plane(buffer + offset)),
        regions: core::array::from_fn(|index| {
            let region = buffer + RAW_REGIONS + RAW_REGION_BYTES * index as u32;
            [doubleword(region), doubleword(region + 8), doubleword(region + 16)]
        }),
        black: unsafe { read_volatile((buffer + RAW_BLACK) as usize as *const [u32; CHANNELS]) },
    }
}

/// The last completed capture of `kind`: its parameters copied to `parameters`, its record block
/// to `block`, and its raw buffer (0xffcfd0).
fn completed(kind: u8, parameters: &mut ParameterBytes, block: Option<&mut [u8; STATISTICS_BLOCK_BYTES]>) -> Option<u32> {
    let mut source = 0u32;
    let block = block.map_or(core::ptr::null_mut(), |block| block.as_mut_ptr());
    let found = entry!(0xff_cfd0, fn(u32, *mut u32, *mut u8, *mut u8) -> u32)(u32::from(kind), &mut source, parameters.as_mut_ptr(), block);
    (found != 0).then_some(source)
}

/// The preparation 0xfeaba8 enters: `store` receives the completed capture's corrected grid and
/// regions, `parameters` its parameters with their areas rescaled. False when the last completed
/// capture is another kind; `parameters` is then untouched.
pub fn prepare_statistics(kind: u8, store: StockBlock, parameters: &mut ParameterBytes) -> bool {
    let Some(source) = completed(kind, parameters, None) else { return false };
    let mut typed = self::parameters(parameters);
    let output = Prepared {
        planes: unsafe { &mut *(store.0 as usize as *mut [Plane; CHANNELS]) },
        regions: unsafe { &mut *((store.0 + 0x1000) as usize as *mut [Region; REGIONS]) },
    };
    prepare(&mut StockCapture, &raw(source), &mut typed, output);
    store_areas(&typed, parameters);
    true
}

/// The configuration of one 0xfeacac call.
struct StockConfiguration<'a> {
    kind: u8,
    parameters: &'a mut ParameterBytes,
    selection: &'a mut SelectionBytes,
    dump: Option<StockBlock>,
    source: u32,
    block: [u8; STATISTICS_BLOCK_BYTES],
}

impl Adjustments for StockConfiguration<'_> {
    fn byte(&self, key: AdjustmentKey) -> u8 { StockCapture.byte(key) }
}

impl PixelGeometry for StockConfiguration<'_> {
    fn shared_divisors(&mut self) -> [u32; 2] { StockCapture.shared_divisors() }

    fn scale_divisors(&mut self, profile: ProfileHandle) -> [u32; 2] { StockCapture.scale_divisors(profile) }

    fn channel_pixels(&mut self, pixels: u32) -> ChannelPixels { StockCapture.channel_pixels(pixels) }

    fn aspect(&mut self, profile: ProfileHandle) -> i32 { StockCapture.aspect(profile) }
}

fn levels(address: u32, reference: i16, profile: ProfileHandle) -> [i32; CHANNELS] {
    let mut levels = [0i32; CHANNELS];
    entry!(address, fn(i32, u32, *mut i32))(i32::from(reference), profile.0, levels.as_mut_ptr());
    levels
}

impl StockConfiguration<'_> {
    fn dump_word(&self, offset: u32, value: u32) {
        if let Some(dump) = self.dump {
            unsafe { write_volatile((dump.0 + offset) as usize as *mut u32, value) }
        }
    }
}

impl ConfigurationPorts for StockConfiguration<'_> {
    fn completed(&mut self) -> Option<(CaptureParameters, CaptureBlock)> {
        self.source = completed(self.kind, self.parameters, Some(&mut self.block))?;
        let block = CaptureBlock { window: area(&self.block, 8), grid: area(&self.block, 0x10) };
        Some((parameters(self.parameters), block))
    }

    fn capture(&mut self, store: RawStore) {
        let [current, _] = store_buffers(store);
        unsafe { copy_nonoverlapping(self.source as usize as *const u8, current as usize as *mut u8, RAW_BYTES) }
    }

    fn keep(&mut self, store: RawStore) {
        let [current, previous] = store_buffers(store);
        unsafe { copy_nonoverlapping(current as usize as *const u8, previous as usize as *mut u8, RAW_BYTES) }
    }

    fn publish(&mut self, publication: &Publication) {
        let previous = if publication.previous { PREVIOUS_WORDS } else { [0, 0] };
        put_word(self.selection, SELECTION_PREVIOUS, previous[0]);
        put_word(self.selection, SELECTION_PREVIOUS + 4, previous[1]);

        let parameters = publication.parameters;
        let mut block = [0u8; PUBLICATION_BYTES];
        entry!(0xed_8af4, fn(*mut u8))(block.as_mut_ptr());
        put_word(&mut block, 0, parameters.profile.0);
        block[4..6].copy_from_slice(&parameters.level.to_le_bytes());
        block[6..8].copy_from_slice(&(parameters.reference as u16).wrapping_add(parameters.reference_span).to_le_bytes());
        put_word(&mut block, 8, parameters.lens);
        put_word(&mut block, 0xc, parameters.lens_reference);
        put_word(&mut block, 0x24, parameters.setting);
        block[0x28..0x2a].copy_from_slice(&(parameters.decade as u16).to_le_bytes());
        block[0x2a..0x2c].copy_from_slice(&parameters.companion.to_le_bytes());

        let buffer = publication.store.map_or(0, |store| store_buffers(store)[0]);
        entry!(0xed_b05c, fn(u32, *mut u8, u32, u32, *mut u8, u32, *mut u8))(
            publication.client.0,
            block.as_mut_ptr(),
            buffer,
            0,
            self.block.as_mut_ptr(),
            u32::from(publication.first),
            self.selection.as_mut_ptr(),
        );
    }

    fn raw_pair(&mut self, store: RawStore) -> RawPair<'_> {
        let [current, previous] = store_buffers(store);
        let area = |buffer: u32, index: u32| buffer + RAW_PLANES + PLANE_BYTES * index;
        RawPair {
            current: [0, 1, 2, 3].map(|index| plane_mut(area(current, index))),
            current_black: black_mut(current + RAW_BLACK),
            previous: [0, 1, 2, 3].map(|index| plane(area(previous, index))),
            previous_black: unsafe { &*((previous + RAW_BLACK) as usize as *const [u32; CHANNELS]) },
        }
    }

    fn raw(&mut self, store: RawStore) -> RawStatistics<'_> { raw(store_buffers(store)[0]) }

    fn record_levels(&mut self, record: &LevelRecord) {
        let [first, second, third, fourth] = record.output.map(|level| level as u32);
        for (offset, value) in [(0, second), (4, first), (8, fourth), (0xc, third)] {
            self.dump_word(DUMP_LEVELS + offset, value);
        }
        let signed = record.signed;
        self.dump_word(DUMP_LEVELS + 0x20, ((signed[1].wrapping_add(signed[3])) / 2) as u32);
        self.dump_word(DUMP_LEVELS + 0x24, signed[0] as u32);
        self.dump_word(DUMP_LEVELS + 0x28, signed[2] as u32);
    }

    fn record_statistics(&mut self, record: &StatisticsRecord) {
        let Some(dump) = self.dump else { return };
        let base = dump.0.wrapping_add(DUMP_STATISTICS);
        if base == 0 {
            return;
        }
        self.dump_word(DUMP_STATISTICS + 0xc, record.grid.height);
        self.dump_word(DUMP_STATISTICS + 0x10, record.grid.width);
        let [first, second, third, fourth] = record.black;
        for (index, value) in [second, first, fourth, third].into_iter().enumerate() {
            let at = base + 0x101c + 0x40 * index as u32;
            unsafe { write_bytes(at as usize as *mut u8, 0, 0x40) }
            self.dump_word(DUMP_STATISTICS + 0x101c + 0x40 * index as u32, value);
        }
    }

    fn update(&mut self, parameters: &CaptureParameters) { store_areas(parameters, self.parameters) }

    fn output_levels(&mut self, reference: i16, profile: ProfileHandle) -> [i32; CHANNELS] { levels(0xed_ba78, reference, profile) }

    fn signed_levels(&mut self, reference: i16, profile: ProfileHandle) -> [i32; CHANNELS] { levels(0xed_b0ac, reference, profile) }

    fn selection_status(&mut self, reference: i16, profile: ProfileHandle) -> u32 { entry!(0xed_b9fc, fn(i32, u32) -> u32)(i32::from(reference), profile.0) }

    fn level_index(&mut self, decade: i16) -> u32 {
        let encoded = entry!(0xef_743c, fn(i32, i32) -> u32)(i32::from(decade), 0);
        entry!(0xef_73e4, fn(u32) -> u32)(encoded)
    }

    fn calibrated_level(&mut self, step: u32) -> i16 { entry!(0x77_2f28, fn(u32) -> u16)(step) as i16 }

    fn linearized(&mut self, level: i16) -> u32 { entry!(0xee_9ab0, fn(i32) -> u32)(i32::from(level)) }
}

/// The configuration 0xfeacac enters: `output` receives the corrected grid, `parameters` the
/// completed capture's parameters with their extent and grid rescaled, `selection` the
/// previous-buffer words, `dump` the levels and statistics used. False when the last completed
/// capture is another kind.
pub fn configure_processing(
    configuration: &mut CaptureConfiguration,
    kind: u8,
    output: Option<StockBlock>,
    parameters: &mut ParameterBytes,
    client: u32,
    dump: Option<StockBlock>,
    selection: &mut SelectionBytes,
) -> bool {
    let request = Selection { difference: word(selection, SELECTION_MODE) == DIFFERENCE_MODE, level_correction: word(selection, SELECTION_VALUE) == 0 };
    let client = (client != 0).then_some(ClientWord(client));
    let output = output.map(|output| GridBuffer {
        planes: unsafe { &mut *(output.0 as usize as *mut [Plane; CHANNELS]) },
        cleared: unsafe { &mut *((output.0 + 0x1000) as usize as *mut [u16; 8]) },
        totals: unsafe { &mut *((output.0 + 0x1010) as usize as *mut [u32; CHANNELS]) },
    });
    let mut ports = StockConfiguration { kind, parameters, selection, dump, source: 0, block: [0; STATISTICS_BLOCK_BYTES] };
    configuration.configure(&mut ports, request, client, output)
}
