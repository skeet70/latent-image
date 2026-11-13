#![forbid(unsafe_code)]

//! The two scan stages' radial geometry: a flat table, or coefficients from the configuration
//! that the image's radial model turns into a table (0xee6804, 0xee6a9c).

use super::changes::{Mask, Stage, Subscription, Work};
use super::windows::ScanSide;
use super::{ProfileGeometry, Tables};
use crate::memory::{Location, Memory};
use crate::ports::{ConfigOffset, Ports, RadialBuffer as RadialTable};
use crate::profile::{PipelineGroup, ProfileHandle};
use crate::stock::layout::{counters, parameters, policy, status, work::scan_geometry as block};

const SCAN_FLAGS: ConfigOffset = ConfigOffset(0xc0);
/// Set in the scan flags when tracked scan indices may use radial geometry.
const TRACKED_RADIAL: u8 = 0x40;
const DIRECT_RADIAL: ConfigOffset = ConfigOffset(0x3e0009);
const RADIAL_ENABLED: ConfigOffset = ConfigOffset(0x403eaf);
/// One bit per scan index.
const RADIAL_INDICES: u32 = 0x3e3a50;
const UNSCANNED_GROUP: PipelineGroup = PipelineGroup(13);
const COMPACT_GROUP: PipelineGroup = PipelineGroup(5);
const COEFFICIENTS: usize = 31;
const FLAT_GAIN: u16 = 0x1000;
const FLAT_POINT: u32 = 0x1_0000;
/// The pixel clock pair every radial model call takes.
const CLOCKS: (u32, u32) = (0x1320, 3264);
const REFUSED_OPTION: u32 = 49;
const SECOND_OPTION: u32 = 50;

/// Where each kind of coefficient set starts in the configuration, and its two gain offsets.
const fn coefficient_layout(kind: u32) -> Option<(u32, u32, u32, u32)> {
    match kind {
        0 => Some((0x401200, 101, 0x403f00, 0x403f02)),
        1 => Some((0x402400, 101, 0x404500, 0x404502)),
        2 | 3 => Some((0x403900, 97, 0x40395d, 0x40395f)),
        4 => Some((0x404b00, 97, 0x404b5d, 0x404b5f)),
        _ => None,
    }
}

/// The largest power-of-two block exponent, at most 4, that divides `size` (0xedbdf8).
const fn block_exponent(size: u32) -> u32 {
    if size & 0x7f == 0 {
        return 4;
    }
    let mut exponent = 3;
    while exponent > 0 && size % (8 << exponent) != 0 {
        exponent -= 1;
    }
    exponent
}

/// 0xedbe3c.
const fn aligned_exponent(start: u32, size: u32, step: u32) -> u32 {
    let mut exponent = start;
    loop {
        let blocks = (size.wrapping_sub(1).wrapping_add(8 << exponent)) / (8 << exponent);
        let phase = step.wrapping_mul(blocks) & 7;
        if (1u32 << phase) & 0xe1 != 0 || exponent == 0 {
            return exponent;
        }
        exponent -= 1;
    }
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xee6804.
    pub(crate) fn update_primary_scan_geometry(&mut self) { self.update_scan_geometry(ScanSide::Primary) }

    /// 0xee6a9c.
    pub(crate) fn update_secondary_scan_geometry(&mut self) { self.update_scan_geometry(ScanSide::Secondary) }

    /// Whether the scan index may use radial geometry (0xeddaec).
    fn radial_index(&mut self, index: u32) -> bool {
        let profile = ProfileHandle(self.memory.read_u32(status::SCAN_PROFILE));
        if self.profiles.pipeline_group(profile) == UNSCANNED_GROUP
            || self.ports.byte(SCAN_FLAGS) & TRACKED_RADIAL == 0
            || self.ports.byte(RADIAL_ENABLED) == 0
        {
            return false;
        }
        let bits = self.ports.byte(ConfigOffset(RADIAL_INDICES + (index >> 3)));
        bits >> (index & 7) & 1 != 0 && self.memory.read_u32(status::TRACKING_HELD) == 0
    }

    fn update_scan_geometry(&mut self, side: ScanSide) {
        let (work, stage, subscription, apply) = match side {
            ScanSide::Primary => (Work::PrimaryScan, Stage::PrimaryScan, Subscription::PrimaryScan, 0x100),
            ScanSide::Secondary => (Work::SecondaryScan, Stage::SecondaryScan, Subscription::SecondaryScan, 0x200),
        };
        let mut radial = self.radial_index(self.memory.read_u32(counters::INDEX));
        if !self.requested(work) {
            return;
        }
        let flags = self.ports.byte(SCAN_FLAGS);
        let profile = ProfileHandle(self.memory.read_u32(status::SCAN_PROFILE));
        if self.ports.byte(DIRECT_RADIAL) != 0 && self.memory.read_u32(parameters::MODE_WORD) == 1 {
            let option = self.memory.read_u32(status::SCAN_INDEX);
            if flags & TRACKED_RADIAL == 0 || option == 0 {
                self.finish(stage);
                self.flatten(side, profile);
            } else {
                self.enable(stage);
                self.load_direct(side, profile, option);
            }
            self.add_to_mask(Mask::Apply, apply);
        } else if self.subscribed(subscription) {
            if self.memory.read_u32(counters::TRACKING) == 1 {
                loop {
                    let index = self.memory.read_u32(counters::INDEX);
                    let tracked = self.memory.read_u32(status::TRACKED_INDEX);
                    if index == tracked {
                        break;
                    }
                    let next = if index < tracked { index + 1 } else { index - 1 };
                    self.memory.write_u32(counters::INDEX, next);
                    radial = self.radial_index(next);
                    if radial {
                        break;
                    }
                }
            }
            if radial {
                self.enable(stage);
                self.load_tracked(side, profile);
            } else {
                self.flatten(side, profile);
                self.finish(stage);
            }
            self.add_to_mask(Mask::Apply, apply);
        }
        self.clear_request(work);
    }

    fn scan_record(side: ScanSide) -> Location {
        match side {
            ScanSide::Primary => crate::stock::layout::work::records::PRIMARY_SCAN,
            ScanSide::Secondary => crate::stock::layout::work::records::SECONDARY_SCAN,
        }
    }

    /// The scan's pixel scales after the class multipliers (0xedbfd4).
    fn pixel_scales(&self, profile: ProfileHandle) -> [u16; 2] {
        let class = self.profiles.class_of(profile);
        let first = if class.first_flag == 0 && class.second_flag == 0 {
            self.profiles.selection_multiplier(profile)
        } else if class.first_flag != 0 {
            2
        } else {
            1
        };
        let second = if class.second_flag != 0 { 2 } else { 1 };
        [(class.first_scale as u32 * first) as u16, (class.second_scale as u32 * second) as u16]
    }

    fn store_shift(&mut self, side: ScanSide, start: u32) {
        let record = Self::scan_record(side);
        let size = self.memory.read_u16(record.plus(0x14)) as u32;
        let step = self.memory.read_u16(record.plus(0x16)) as u32;
        self.memory.write_u32(record.plus(0x28), aligned_exponent(start, size, step));
    }

    /// A flat radial table (0xedd650, 0xedd7ec).
    fn flatten(&mut self, side: ScanSide, profile: ProfileHandle) {
        let at = block::base(side == ScanSide::Secondary);
        let record = Self::scan_record(side);
        self.memory.write_u16(at, FLAT_GAIN);
        self.memory.write_u16(at.plus(2), FLAT_GAIN);
        self.memory.write_u32(at.plus(4), self.memory.read_u32(record.plus(0xc)));
        self.memory.write_u32(at.plus(8), self.memory.read_u32(record.plus(0x10)));
        self.memory.write_u16(at.plus(0xc), 1);
        self.memory.write_u16(at.plus(0xe), 1);
        let [first, second] = self.pixel_scales(profile);
        self.memory.write_u8(at.plus(0x10), first as u8);
        self.memory.write_u8(at.plus(0x11), second as u8);
        for index in 0..COEFFICIENTS as u16 {
            self.memory.write_u32(at.plus(0x14 + index * 4), FLAT_POINT);
        }
        let size = self.memory.read_u16(record.plus(0x14)) as u32;
        self.store_shift(side, block_exponent(size));
    }

    /// 0xedd70c, 0xedd8a8.
    fn load_direct(&mut self, side: ScanSide, profile: ProfileHandle, option: u32) {
        match option {
            1..=6 => match self.radial_table(profile, 4, option - 1, side) {
                Some(table) => self.store_radial(side, &table),
                None => self.flatten(side, profile),
            },
            _ => self.flatten(side, profile),
        }
    }

    /// 0xedd77c, 0xedd918.
    fn load_tracked(&mut self, side: ScanSide, profile: ProfileHandle) {
        let kind = if self.profiles.entry(profile).kind.0 != 0 {
            3
        } else if self.memory.read_u16(status::SCAN_LIMIT) & 2 != 0 {
            0
        } else {
            match self.memory.read_u32(status::AREA.plus(0x10)) {
                REFUSED_OPTION => 0,
                SECOND_OPTION => 1,
                _ => 2,
            }
        };
        let index = self.memory.read_u32(counters::INDEX);
        match self.radial_table(profile, kind, index, side) {
            Some(table) => self.store_radial(side, &table),
            None => self.flatten(side, profile),
        }
    }

    /// The configured coefficients of `kind` at `index` through the radial model (0xedc2f0).
    fn radial_table(&mut self, profile: ProfileHandle, kind: u32, index: u32, side: ScanSide) -> Option<RadialTable> {
        let (base, stride, first_gain, second_gain) = coefficient_layout(kind)?;
        let row = index.wrapping_mul(stride);
        let gain_row = if stride == 101 { index.wrapping_mul(4) } else { row };
        let pair_at = policy::SCAN_PAIRS[side as usize];
        let pair = [self.memory.read_u32(pair_at), self.memory.read_u32(pair_at.plus(4))];
        if pair == [0, 0] {
            return None;
        }
        let mut table = RadialTable::default();
        let mut offset = base.wrapping_add(row);
        for point in table.points.iter_mut() {
            let bytes = [0, 1, 2].map(|byte| self.ports.byte(ConfigOffset(offset + byte)) as u32);
            *point = bytes[0] | bytes[1] << 8 | bytes[2] << 16;
            offset += 3;
        }
        let mut model = self.ports.radial_model(&table.points);
        table.scales = self.pixel_scales(profile);
        if self.profiles.pipeline_group(profile) == COMPACT_GROUP {
            let compact = self.ports.radial_model(&table.points);
            if !self.ports.fit_compact(CLOCKS, &compact, &mut table) {
                return None;
            }
        } else {
            table.gains = [
                self.ports.halfword(ConfigOffset(first_gain.wrapping_add(gain_row))),
                self.ports.halfword(ConfigOffset(second_gain.wrapping_add(gain_row))),
            ];
            table.bounds[0] = self.ports.radial_bound(CLOCKS);
            let centre = self.ports.radial_centre(&mut model);
            table.bounds[1] = self.ports.radial_extent(CLOCKS, centre, table.bounds[0], table.gains[0]);
        }
        table.shift = self.ports.radial_table(CLOCKS, pair, &model, table.bounds[1], table.scales, table.gains[1]);
        Some(table)
    }

    /// 0xedbf0c.
    fn store_radial(&mut self, side: ScanSide, table: &RadialTable) {
        let at = block::base(side == ScanSide::Secondary);
        let record = Self::scan_record(side);
        self.memory.write_u16(at, table.gains[0]);
        self.memory.write_u16(at.plus(2), table.gains[1]);
        self.memory.write_u32(at.plus(4), self.memory.read_u32(record.plus(0xc)));
        self.memory.write_u32(at.plus(8), self.memory.read_u32(record.plus(0x10)));
        self.memory.write_u16(at.plus(0xc), table.bounds[0]);
        self.memory.write_u16(at.plus(0xe), table.bounds[1]);
        self.memory.write_u8(at.plus(0x10), table.scales[0] as u8);
        self.memory.write_u8(at.plus(0x11), table.scales[1] as u8);
        for (index, &point) in table.points.iter().enumerate() {
            self.memory.write_u32(at.plus(0x14 + index as u16 * 4), point);
        }
        self.store_shift(side, table.shift as u32);
    }
}

