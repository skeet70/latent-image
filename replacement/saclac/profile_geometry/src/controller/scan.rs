#![forbid(unsafe_code)]

//! Filling the pipeline's scan objects: rebuilding the controller's from the status snapshot
//! (0xee5684, 0xedccd8), and exporting a caller's processing request (0xeef3f0).

use super::changes::Work;
use super::{ProfileGeometry, Status, Tables};
use crate::memory::{Location, Memory};
use crate::numeric::log2_offset;
use crate::ports::{Ports, ScanExport, ScanSetting, Stages, StockData};
use crate::profile::{ProfileHandle, ProfileKind};
use crate::stock::layout::{callbacks, controller as field, parameters, policy, selection, status, work};

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
/// 0xeef614.
const CHANNEL_CODES: [u16; 10] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9];
const NO_CHANNEL: u8 = 0x80;

/// The record processing kind and its companions (0xeeee20).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct RecordProcessing {
    kind: u32,
    second: u32,
    third: u32,
}

/// The processing request a caller exports (0xeef1e4); each field is converted to its scan
/// setting as the controller converts the status snapshot's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessingRequest {
    pub option: u32,
    pub output_mode: u32,
    /// Height, then width.
    pub dimensions: [u32; 2],
    pub format: u32,
    pub selection: u32,
    pub code: u32,
    pub geometry_kind: u32,
    pub geometry_enabled: u32,
    pub query_mode: u32,
    /// Primary, then secondary.
    pub record_kinds: [u32; 2],
    pub processing_level: u32,
    pub profile: ProfileHandle,
    pub record_kind: u8,
    pub limit: u16,
    pub rate_companion: u32,
    pub toggle: u32,
    /// Bit 1 selects the flagged processing option.
    pub option_flags: u16,
    pub byte70: u8,
    pub channel: u8,
}

/// 0xeee4d4.
fn processing_option(option: u32, output_mode: u32, flags: u16) -> u32 {
    if output_mode == 1 {
        MONOCHROME_OPTION
    } else if flags & 2 != 0 {
        FLAGGED_OPTION
    } else {
        PROCESSING_OPTIONS.get(option as usize).map_or(0, |&code| code as u32)
    }
}

/// 0xeeeccc.
const fn geometry_kind(kind: u32) -> u32 {
    match kind {
        0..=3 => kind,
        4 => 5,
        5 => 4,
        _ => 0,
    }
}

/// 0xeeed6c.
const fn toggle(value: u32) -> u32 { (value == 2) as u32 }

/// 0xeeea84.
const fn query_kind(selection: u32) -> u32 { if selection <= 5 { selection } else { 1 } }

/// 0xeee958.
const fn format(format: u32) -> u32 { if format <= 12 { format } else { 0 } }

/// 0xeeeb24.
const fn query_mode(mode: u32) -> u32 {
    match mode {
        1 => 2,
        2 => 4,
        _ => 0,
    }
}

/// 0xeeeb74, 0xeeec00.
const fn record_kind(value: u32) -> u32 {
    match value {
        0 => 4,
        1 => 3,
        2 => 2,
        3 => 1,
        4 => 0,
        _ => 2,
    }
}

/// 0xeeec8c.
const fn geometry_enabled(value: u32) -> u32 { (value == 1) as u32 }

/// 0xeeef38.
const fn dimensions(output_mode: u32, [height, width]: [u32; 2]) -> u32 {
    match (output_mode, height, width) {
        (1, 240, 320) => 0,
        (1, 720, 1280) => 2,
        (1, 1080, 1920) => 3,
        _ => 1,
    }
}

/// 0xeef07c.
fn processing_level(level: u32) -> u32 { PROCESSING_LEVELS.get(level as usize).map_or(0, |&code| code as u32) }

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
        let scan = policy::SCAN;
        let profile = self.current_profile();
        let output_mode = self.status_word(0xc);
        let option = processing_option(self.status_word(0x10), output_mode, self.memory.read_u16(status::SCAN_LIMIT));
        self.ports.set(scan, ScanSetting::ProcessingOption(option));
        self.ports.set(scan, ScanSetting::OutputMode(output_mode));
        self.ports.set(scan, ScanSetting::ScanOption(self.memory.read_u32(field::SCAN_OPTION) != 0));
        self.ports.set(scan, ScanSetting::GeometryKind(geometry_kind(self.status_word(0x18))));
        let processing = self.record_processing(profile, self.memory.read_u8(field::RECORD_KIND));
        self.ports.set(scan, ScanSetting::RecordProcessing(processing.kind));
        self.ports.set(scan, ScanSetting::RecordProcessingThird(processing.third));
        self.ports.set(scan, ScanSetting::RecordProcessingSecond(processing.second));
        self.ports.set(scan, ScanSetting::Toggle(toggle(self.status_word(0x54))));
        let selection = self.scan_selection(profile);
        self.ports.set(scan, ScanSetting::QueryKind(query_kind(selection)));
        let format = format(self.status_word(0x5c));
        self.ports.set(scan, ScanSetting::Format(format));
        let code = self.record_code(self.status_word(0x48));
        self.ports.set(scan, ScanSetting::Code(code));
        self.ports.set(scan, ScanSetting::QueryMode(query_mode(self.status_word(0x64))));
        self.ports.set(scan, ScanSetting::PrimaryRecordKind(record_kind(self.status_word(0x68))));
        self.ports.set(scan, ScanSetting::SecondaryRecordKind(record_kind(self.status_word(0x6c))));
        let channel = self.channel_index(self.memory.read_u32(parameters::INDEX_SCALE));
        self.ports.set(scan, ScanSetting::Channel(channel));
        self.ports.set(scan, ScanSetting::GeometryEnabled(geometry_enabled(self.status_word(0x14))));
        self.ports.set(scan, ScanSetting::FormatCompanion(format));
        let sum = self.geometry_offset_sum(profile, selection);
        self.ports.set(scan, ScanSetting::OffsetSum(sum));
        self.ports.set(scan, ScanSetting::OffsetSumLow(sum as u16));
        let rate_code = self.ports.rate_code(sum);
        let rate = rate_code.wrapping_sub(3).try_into().ok().and_then(|index: usize| SCAN_RATES.get(index)).map_or(DEFAULT_SCAN_RATE, |&rate| rate as u32);
        self.ports.set(scan, ScanSetting::ScanRate(rate));
        let limit = self.memory.read_i16(parameters::SECONDARY_LIMIT);
        self.ports.set(scan, ScanSetting::Limit(limit as u16));
        let dimensions = dimensions(output_mode, [self.status_word(0x1c), self.status_word(0x20)]);
        self.ports.set(scan, ScanSetting::Dimensions(dimensions));
        self.ports.set(scan, ScanSetting::Fixed(2));
        let tenths = self.ports.limit_tenths(limit);
        self.ports.set(scan, ScanSetting::LimitTenths(tenths));
        self.ports.set(scan, ScanSetting::Rate(self.memory.read_u16(status::RATE)));
        self.ports.set(scan, ScanSetting::RateCompanion(self.status_word(0xa4)));
        self.ports.set(scan, ScanSetting::Byte70(self.memory.read_u8(status::AREA.plus(0x70))));
        self.ports.set(scan, ScanSetting::ProcessingLevel(processing_level(self.status_word(0xa8))));
        self.ports.set(scan, ScanSetting::AreaPresent(self.status_word(0x0) != 0));
        self.ports.set(scan, ScanSetting::FirstOf(u32::from(self.status_word(0xb0) == 0)));
        let split = if UNSPLIT_KINDS.contains(&processing.kind) || self.status_word(0xb4) != 1 {
            0
        } else if self.status_word(0xb8) == 0 {
            1
        } else {
            2
        };
        self.ports.set(scan, ScanSetting::Split(split));
        self.ports.set(scan, ScanSetting::Multiple(self.memory.read_u8(status::AREA.plus(0xbc)) > 1));
        let [first, second, third] = self.white_balance();
        self.ports.set(scan, ScanSetting::WhiteBalance([second, first, third]));
        let output = self.ports.output_level();
        self.ports.set(scan, ScanSetting::OutputLevel(output));
        let output_kind = self.output_kind();
        self.ports.compile_scan_configuration(output_kind);
        self.apply_operation_callbacks();
        let stages = self.memory.read_u32(policy::RESTORED_STAGES);
        self.ports.attach_scan_configuration(policy::CONFIGURATION, Stages(stages));
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
        if self.memory.read_u32(callbacks::ENABLED) != 1 {
            return;
        }
        self.ports.apply_selection(selection::BLOCK, policy::CONFIGURATION);
    }

    /// Fills the export scan object from `request` and writes its four parts into `out`
    /// (0xeef3f0).
    pub fn export_processing_request(&mut self, request: &ProcessingRequest, out: StockData) {
        let scan = callbacks::EXPORT_SCAN;
        self.fill_export_scan(scan, request);
        for part in ScanExport::ALL {
            self.ports.export_scan(scan, part, out.plus(part.offset()));
        }
    }

    /// 0xeef1e4.
    fn fill_export_scan(&mut self, scan: Location, request: &ProcessingRequest) {
        self.ports.clear_scan(scan);
        self.ports.default_scan(scan);
        self.ports.set(scan, ScanSetting::OutputMode(request.output_mode));
        let option = processing_option(request.option, request.output_mode, request.option_flags);
        self.ports.set(scan, ScanSetting::ProcessingOption(option));
        self.ports.set(scan, ScanSetting::GeometryKind(geometry_kind(request.geometry_kind)));
        let processing = self.record_processing(request.profile, request.record_kind);
        self.ports.set(scan, ScanSetting::RecordProcessing(processing.kind));
        self.ports.set(scan, ScanSetting::Dimensions(dimensions(request.output_mode, request.dimensions)));
        self.ports.set(scan, ScanSetting::QueryKind(query_kind(request.selection)));
        let format = format(request.format);
        self.ports.set(scan, ScanSetting::Format(format));
        self.ports.set(scan, ScanSetting::Code(self.record_code(request.code)));
        self.ports.set(scan, ScanSetting::QueryMode(query_mode(request.query_mode)));
        self.ports.set(scan, ScanSetting::PrimaryRecordKind(record_kind(request.record_kinds[0])));
        self.ports.set(scan, ScanSetting::SecondaryRecordKind(record_kind(request.record_kinds[1])));
        self.ports.set(scan, ScanSetting::GeometryEnabled(geometry_enabled(request.geometry_enabled)));
        self.ports.set(scan, ScanSetting::FormatCompanion(format));
        self.ports.set(scan, ScanSetting::Limit(request.limit));
        self.ports.set(scan, ScanSetting::RateCompanion(request.rate_companion));
        self.ports.set(scan, ScanSetting::Toggle(toggle(request.toggle)));
        self.ports.set(scan, ScanSetting::Byte70(request.byte70));
        self.ports.set(scan, ScanSetting::ProcessingLevel(processing_level(request.processing_level)));
        let last = self.ports.channel_count().wrapping_sub(1);
        let channel = if request.channel <= last { request.channel.wrapping_add(1) } else { NO_CHANNEL };
        let code = self.ports.channel_code(channel as u32, &CHANNEL_CODES);
        self.ports.set(scan, ScanSetting::ChannelCode(code as u32));
    }
}
