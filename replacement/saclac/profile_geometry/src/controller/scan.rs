#![forbid(unsafe_code)]

//! Rebuilding the pipeline's scan configuration from the status snapshot (0xee5684, 0xedccd8).

use super::changes::Work;
use super::{ProfileGeometry, Status, Tables};
use crate::memory::Memory;
use crate::numeric::log2_offset;
use crate::ports::{Ports, ScanSetting};
use crate::profile::{ProfileHandle, ProfileKind};
use crate::stock::layout::{controller as field, parameters, policy, status, work};

/// The pipeline option codes of the snapshot's processing option (0xeee4d4); beyond the table
/// the option is 0.
const PROCESSING_OPTIONS: [u8; 57] = [
    0, 5, 6, 24, 7, 8, 9, 10, 48, 49, 11, 13, 12, 12, 51, 50, 14, 47, 16, 52, 53, 17, 54, 55, 18, 19, 56, 57, 20, 21, 22,
    58, 23, 62, 64, 65, 66, 67, 68, 69, 70, 71, 72, 37, 33, 34, 35, 36, 73, 25, 26, 27, 28, 29, 29, 29, 29,
];
const MONOCHROME_OPTION: u32 = 44;
const FLAGGED_OPTION: u32 = 63;
/// The pipeline level codes of the snapshot's processing level (0xeef07c).
const PROCESSING_LEVELS: [u8; 15] = [0, 1, 2, 12, 3, 4, 5, 6, 9, 11, 7, 8, 10, 0, 13];
/// The scan rates of the decoded rate codes 3 to 10 (0xeeefcc); other codes scan at 8.
const SCAN_RATES: [u8; 8] = [10, 13, 16, 19, 22, 25, 27, 28];
const DEFAULT_SCAN_RATE: u32 = 8;
const RATE_OFFSET: i16 = 664;
/// The processing kinds without a split output.
const UNSPLIT_KINDS: [u32; 3] = [5, 6, 7];

/// The record processing kind and its companions (0xeeee20).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecordProcessing {
    kind: u32,
    second: u32,
    third: u32,
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xee5684.
    pub(crate) fn rebuild_scan_configuration(&mut self) -> Status {
        if !self.requested(Work::ScanConfiguration) {
            return Ok(());
        }
        if self.scan_claimed() {
            self.configure_scan();
            self.memory.write_u32(work::SCAN_REBUILT, 1);
        }
        self.clear_request(Work::ScanConfiguration);
        Ok(())
    }

    fn status_word(&self, offset: u16) -> u32 { self.memory.read_u32(status::AREA.plus(offset)) }

    /// 0xeeee20.
    fn record_processing(&self, profile: ProfileHandle, record_kind: u8) -> RecordProcessing {
        let entry = self.profiles.entry(profile);
        let plain = RecordProcessing { kind: 1, second: 0, third: 0 };
        match entry.kind.0 {
            0 => {
                let third = match entry.feature {
                    1 if record_kind == 1 => 3,
                    1 => 1,
                    3 => 2,
                    _ => 0,
                };
                RecordProcessing { kind: 7, third, ..plain }
            }
            2 => RecordProcessing { second: u32::from(self.profiles.flags_low(profile) != 0), ..plain },
            3 => RecordProcessing { kind: 5, ..plain },
            5 => RecordProcessing { kind: 3, ..plain },
            _ => plain,
        }
    }

    /// The scan rate selection the snapshot asks for, or the profile's thresholds when its kind is
    /// zero (0xedc678).
    fn scan_selection(&mut self, profile: ProfileHandle) -> u32 {
        if self.profiles.entry(profile).kind != ProfileKind(0) {
            return self.status_word(0x60);
        }
        let first = self.memory.read_i16(parameters::SCAN_FIRST);
        let second = self.memory.read_i16(parameters::SCAN_SECOND);
        self.ports.encode_thresholds(first, second)
    }

    /// 0xedc0e8.
    fn geometry_offset_sum(&mut self, profile: ProfileHandle, selection: u32) -> u32 {
        let limit = self.memory.read_i16(parameters::SECONDARY_LIMIT);
        let sum = (self.profiles.aspect_value(profile) as i32)
            .wrapping_add(self.ports.converted_decade(selection) as i32)
            .wrapping_add(limit as i32) as u16;
        let rate = log2_offset(self.profiles.tables(), self.memory.read_u16(status::RATE) as u32, RATE_OFFSET);
        sum.wrapping_add(rate as u16) as i16 as i32 as u32
    }

    /// 0xedc0b8.
    fn channel_index(&mut self, index_scale: u32) -> u32 {
        let index = index_scale as i32;
        if index < 0 || index > self.ports.channel_count() as i32 - 1 {
            0x80
        } else {
            (index as u32 + 1) & 0xff
        }
    }

    /// The pipeline code of a status code (0xeeedac).
    fn record_code(&self, code: u32) -> u32 {
        let tables = self.profiles.tables();
        if code == 0 {
            return tables.record_code(0)[1] as u32;
        }
        (1..62).find(|&index| tables.record_code(index)[0] as u32 == code).map_or(0, |index| tables.record_code(index)[1] as u32)
    }

    /// 0xedccd8.
    fn configure_scan(&mut self) {
        let profile = self.current_profile();
        let output_mode = self.status_word(0xc);
        let option = match self.status_word(0x10) {
            _ if output_mode == 1 => MONOCHROME_OPTION,
            _ if self.memory.read_u16(status::SCAN_LIMIT) & 2 != 0 => FLAGGED_OPTION,
            option => PROCESSING_OPTIONS.get(option as usize).map_or(0, |&code| code as u32),
        };
        self.ports.set(ScanSetting::ProcessingOption(option));
        self.ports.set(ScanSetting::OutputMode(output_mode));
        self.ports.set(ScanSetting::ScanOption(self.memory.read_u32(field::SCAN_OPTION) != 0));
        let geometry_kind = match self.status_word(0x18) {
            kind @ 0..=3 => kind,
            4 => 5,
            5 => 4,
            _ => 0,
        };
        self.ports.set(ScanSetting::GeometryKind(geometry_kind));
        let processing = self.record_processing(profile, self.memory.read_u8(field::RECORD_KIND));
        self.ports.set(ScanSetting::RecordProcessing(processing.kind));
        self.ports.set(ScanSetting::RecordProcessingThird(processing.third));
        self.ports.set(ScanSetting::RecordProcessingSecond(processing.second));
        let toggle = match self.status_word(0x54) {
            2 => 1,
            _ => 0,
        };
        self.ports.set(ScanSetting::Toggle(toggle));
        let selection = self.scan_selection(profile);
        let query_kind = if selection <= 5 { selection } else { 1 };
        self.ports.set(ScanSetting::QueryKind(query_kind));
        let format = self.status_word(0x5c);
        let format = if format <= 12 { format } else { 0 };
        self.ports.set(ScanSetting::Format(format));
        let code = self.record_code(self.status_word(0x48));
        self.ports.set(ScanSetting::Code(code));
        let query_mode = match self.status_word(0x64) {
            0 => 0,
            1 => 2,
            2 => 4,
            _ => 0,
        };
        self.ports.set(ScanSetting::QueryMode(query_mode));
        let record_kind = |value: u32| match value {
            0 => 4,
            1 => 3,
            2 => 2,
            3 => 1,
            4 => 0,
            _ => 2,
        };
        self.ports.set(ScanSetting::PrimaryRecordKind(record_kind(self.status_word(0x68))));
        self.ports.set(ScanSetting::SecondaryRecordKind(record_kind(self.status_word(0x6c))));
        let channel = self.channel_index(self.memory.read_u32(parameters::INDEX_SCALE));
        self.ports.set(ScanSetting::Channel(channel));
        self.ports.set(ScanSetting::GeometryEnabled(u32::from(self.status_word(0x14) == 1)));
        self.ports.set(ScanSetting::FormatCompanion(format));
        let sum = self.geometry_offset_sum(profile, selection);
        self.ports.set(ScanSetting::OffsetSum(sum));
        self.ports.set(ScanSetting::OffsetSumLow(sum as u16));
        let rate_code = self.ports.rate_code(sum);
        let rate = rate_code.wrapping_sub(3).try_into().ok().and_then(|index: usize| SCAN_RATES.get(index)).map_or(DEFAULT_SCAN_RATE, |&rate| rate as u32);
        self.ports.set(ScanSetting::ScanRate(rate));
        let limit = self.memory.read_i16(parameters::SECONDARY_LIMIT);
        self.ports.set(ScanSetting::Limit(limit as u16));
        let dimensions = match (output_mode, self.status_word(0x1c), self.status_word(0x20)) {
            (1, 0xf0, 0x140) => 0,
            (1, 0x2d0, 0x500) => 2,
            (1, 0x438, 0x780) => 3,
            _ => 1,
        };
        self.ports.set(ScanSetting::Dimensions(dimensions));
        self.ports.set(ScanSetting::Fixed(2));
        let tenths = self.ports.limit_tenths(limit);
        self.ports.set(ScanSetting::LimitTenths(tenths));
        self.ports.set(ScanSetting::Rate(self.memory.read_u16(status::RATE)));
        self.ports.set(ScanSetting::RateCompanion(self.status_word(0xa4)));
        self.ports.set(ScanSetting::Byte70(self.memory.read_u8(status::AREA.plus(0x70))));
        let level = self.status_word(0xa8);
        let level = PROCESSING_LEVELS.get(level as usize).map_or(0, |&code| code as u32);
        self.ports.set(ScanSetting::ProcessingLevel(level));
        self.ports.set(ScanSetting::AreaPresent(self.status_word(0x0) != 0));
        self.ports.set(ScanSetting::FirstOf(u32::from(self.status_word(0xb0) == 0)));
        let split = if UNSPLIT_KINDS.contains(&processing.kind) || self.status_word(0xb4) != 1 {
            0
        } else if self.status_word(0xb8) == 0 {
            1
        } else {
            2
        };
        self.ports.set(ScanSetting::Split(split));
        self.ports.set(ScanSetting::Multiple(self.memory.read_u8(status::AREA.plus(0xbc)) > 1));
        let [first, second, third] = self.white_balance();
        self.ports.set(ScanSetting::WhiteBalance([second, first, third]));
        let output = self.ports.output_level();
        self.ports.set(ScanSetting::OutputLevel(output));
        let output_kind = self.output_kind();
        self.ports.compile_scan_configuration(output_kind);
        self.apply_operation_callbacks();
        let stages = self.memory.read_u32(policy::RESTORED_STAGES);
        self.ports.attach_scan_configuration(policy::CONFIGURATION, crate::ports::Stages(stages));
    }

    /// The output kind the compiled configuration takes (0xedcc60).
    fn output_kind(&mut self) -> u8 {
        let profile = self.current_profile();
        if self.remainder_geometry() {
            let [first, second] = self.class_remainders(profile);
            return first | second << 3;
        }
        self.profiles.class_kind(profile)
    }

    /// 0xeef444.
    fn apply_operation_callbacks(&mut self) {
        if self.memory.read_u32(crate::stock::layout::callbacks::ENABLED) != 1 {
            return;
        }
        self.ports.apply_selection(crate::stock::layout::selection::BLOCK, policy::CONFIGURATION);
    }
}
