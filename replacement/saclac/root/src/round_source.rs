#![forbid(unsafe_code)]

use xm1_focus_measurement_round::{pipeline::TimingWord, profile::{ProfileSelection, RetainedHalfword, RetainedWord}, source::{Ports, ProcessingProfile, ProcessingRequest, SampleProfile, SegmentSettings, SourceProfile, SourcePublication, SourceSettings, SourceStrength, Summary}};
use xm1_saclac_imaging::driver::Isp;
use xm1_saclac_interface::{Calc, Calc1, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::{controller::{Tables, scan::ProcessingRequest as GeometryRequest}, ports::StockData, profile::ProfileHandle};
use xm1_saclac_profile_records::{Memory, StockEntries, SharedHeaderByte, MeasurementSelection, stock::{record::group_00 as r, shared::{group_20 as m, group_23 as publication, group_27 as s}}};
use xm1_saclac_shared::{settings::{self, Adjustments, AdjustmentKey, AspectPair, CaptureMode, WindowScale}, stock::Machine};
use xm1_saclac_shared::lens::LensControl;
use crate::{round_ports::RoundPorts, services::NO_HANDLE};

#[repr(align(4))]
struct AlignedSummary(Summary);

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables>
    Ports for RoundPorts<'_, M, R, E, P, I, T> {
    fn source_profile(&mut self, record: RecordHandle) -> SourceProfile {
        let services = &mut self.services;
        let selection = match services.entries.measurement_selection(record) {
            MeasurementSelection::Unselected => ProfileSelection::Unselected,
            MeasurementSelection::Record => ProfileSelection::Record,
            MeasurementSelection::Other(byte) => ProfileSelection::Other(byte),
        };
        let enabled = services.entries.header_byte(record, SharedHeaderByte::At63);
        let mode = u32::from(services.field(record, &r::AT_0034));
        let pair = services.field(record, &r::AT_0000);
        let window = match CaptureMode::new(mode) {
            None => [0; 2],
            Some(mode) => match settings::capture_window(mode, AspectPair([pair as u16, (pair >> 16) as u16]), &mut services.stock).expect("source capture window has no configured variant") {
                WindowScale::Unscaled => [0; 2],
                WindowScale::Scaled(window) => [window.alternate, window.value],
            },
        };
        let gain = services.shared(record, &s::AT_071C);
        let level = services.shared(record, &s::AT_070E).wrapping_add(services.stock.zone_limit(gain as u8)) as u16;
        SourceProfile {
            measures_source: selection != ProfileSelection::Unselected && enabled == 1,
            keeps_summary: services.shared(record, &m::AT_0148) == 2,
            processing: ProcessingProfile {
                mode_position: RetainedWord(services.selection_mode(record)), selection,
                // The unchecked stock caller leaves these two stack words unwritten when no scaling occurs.
                window: window.map(RetainedWord),
                words: [
                    u32::from(services.field(record, &r::AT_003C)),
                    u32::from(services.field(record, &r::AT_003D)), 0,
                    u32::from(services.field(record, &r::AT_0036)),
                    u32::from(services.field(record, &r::AT_0035)),
                    u32::from(services.field(record, &r::AT_0039)),
                    u32::from(services.field(record, &r::AT_003A)),
                    u32::from(services.field(record, &r::AT_003B)),
                    services.shared(record, &s::AT_0724), u32::from(enabled),
                    services.field(record, &xm1_saclac_profile_records::stock::record::group_13::AT_20B0_4),
                ].map(RetainedWord),
                halfword: RetainedHalfword(services.shared(record, &s::AT_0740)),
                byte: services.shared(record, &s::AT_0720) as u8,
            },
            timing: TimingWord(u32::from(services.field(record, &r::AT_003E))), level,
        }
    }
    fn sample_profile(&mut self, record: RecordHandle) -> SampleProfile {
        let services = &mut self.services;
        let value = u32::from(services.field(record, &r::AT_003D));
        let pair = self.geometry.level_pair(0, value);
        SampleProfile {
            words: [RetainedWord(services.records.chosen_word(record).unwrap_or(0)), RetainedWord(value)],
            halfwords: [RetainedHalfword(self.geometry.record_decade(pair) as u16), RetainedHalfword(services.field(record, &r::AT_001C) as u16)],
            level_offset: services.shared(record, &s::AT_025C) as u16,
            extension: RetainedWord(services.shared(record, &m::CAPTURE_BUFFER).map_or(0, |buffer| buffer.address())),
        }
    }
    fn source_settings(&mut self) -> SourceSettings {
        let stock = &self.services.stock;
        let byte = |key| stock.byte(AdjustmentKey(key));
        SourceSettings {
            segments: [
                [0x16f82, 0x16f83, 0x16f84], [0xc7db, 0xc7dc, 0xc7dd],
                [0xc151, 0xc152, 0xc153], [0xc154, 0xc155, 0xc156],
            ].map(|keys| SegmentSettings { ring_length: byte(keys[0]), newest_weight: byte(keys[1]), gain: byte(keys[2]) }),
            strengths: [0xc7d5, 0xc7d6, 0xc7d7, 0xc7d8, 0xc7d9, 0xc7da].map(byte),
            carries_extension: byte(0xa8) & 0x80 != 0,
        }
    }
    fn processing_summary(&mut self, request: &ProcessingRequest) -> Summary {
        let profile = &request.profile;
        let words = profile.words.map(|word| word.0);
        let request = GeometryRequest {
            option: profile.mode_position.0, output_mode: u32::from(xm1_focus_measurement_round::stock::profile_selection_byte(profile.selection)),
            dimensions: profile.window.map(|word| word.0), format: words[0], selection: words[1], code: words[2],
            geometry_kind: words[3], geometry_enabled: words[4], query_mode: words[5], record_kinds: [words[6], words[7]],
            processing_level: request.timing.0, profile: ProfileHandle(words[8]), record_kind: 1, limit: request.level,
            rate_companion: words[9], toggle: words[10], option_flags: profile.halfword.0, byte70: request.strength.0, channel: profile.byte,
        };
        let mut output = AlignedSummary(Summary::ZERO);
        let address = u32::try_from(output.0.0.as_mut_ptr() as usize).expect("processing export requires a 32-bit guest output address");
        self.geometry.export_processing_request(&request, StockData(address));
        output.0
    }
    fn enable_processing_callbacks(&mut self, enabled: bool) { self.geometry.set_operation_callbacks(u32::from(enabled)); }
    fn published_strength(&mut self) -> SourceStrength { SourceStrength(self.services.shared_bytes(NO_HANDLE, &publication::AT_0194)[12]) }
    fn update_source_values(&mut self, summary: &Summary, means: &Summary) {
        for (base, values) in [(0x016c_05a4, &summary.0), (0x016c_0622, &means.0)] {
            for (index, value) in values.iter().enumerate() { self.services.machine.write_u16(base + 2 * index as u32, *value); }
        }
    }
    fn publish_source(&mut self, record: RecordHandle, value: SourcePublication<'_>) {
        let bytes = xm1_focus_measurement_round::stock::source_publication(value);
        let _refused = self.services.records.write_shared(record, &publication::AT_0194, bytes);
    }
}
