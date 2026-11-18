#![forbid(unsafe_code)]

use xm1_focus_measurement_round::{auxiliary::{Ports, AuxiliaryProfile, AuxiliaryPublication, AuxiliaryReading, AuxiliarySettings, Curve, CurveInputs, RequestProfile, ResourceIndex}, pipeline::{Exposure, PipelineWord, TimingWord}, profile::{ProfileSelection, RetainedHalfword, RetainedWord}};
use xm1_saclac_calc::{adjust::rate_policy, gains::{self, GainRequest}, scene_recognition::Mode};
use xm1_saclac_imaging::{capture::gains::{GainClass, GainSources}, driver::Isp};
use xm1_saclac_interface::{Calc, Calc1, Completed, Completion, ImageProcessor, ImgDc, RecordHandle};
use xm1_saclac_profile_geometry::controller::Tables;
use xm1_saclac_profile_records::{Memory, StockEntries, SharedHeaderByte, stock::{record::group_00 as r, shared::{group_20 as m, group_22 as a, group_27 as s}}};
use xm1_saclac_shared::{settings::{Adjustments, AdjustmentKey}, stock::Machine};
use crate::{capture_gains::Sources, profile_calibration::ProfileCalibration, round_ports::RoundPorts};

impl<M: Machine + Clone, R: Memory, E: StockEntries, P: ImageProcessor + Calc + Calc1 + ImgDc + xm1_saclac_interface::Statistics, I: Isp, T: Tables>
    Ports for RoundPorts<'_, M, R, E, P, I, T> {
    fn reading(&mut self, completed: Completed<'_>) -> Option<AuxiliaryReading> {
        if completed.completion != Completion::Calculation8a || completed.parameters.bytes().len() != 8 { return None; }
        let bytes = completed.parameters.bytes();
        let address = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        if address == 0 { return None; }
        assert!(self.auxiliary_buffer.is_none(), "one round view consumes one auxiliary result");
        self.auxiliary_buffer = Some(address);
        let machine = &self.services.machine;
        Some(AuxiliaryReading {
            value: machine.read_u16(address) as i16,
            levels: core::array::from_fn(|index| machine.read_u16(address + 2 + 2 * index as u32)),
            excluded: machine.read_u32(address + 8) != 0,
        })
    }
    fn auxiliary_settings(&mut self) -> AuxiliarySettings {
        let byte = |key| self.services.stock.byte(AdjustmentKey(key));
        AuxiliarySettings {
            ring_length: byte(0x82b0), newest_weight: byte(0x82b1), gains: [byte(0x82b2), byte(0x82b3)],
            early_readings: byte(0x82b4), follow_band: byte(0x82b5), settled_band: byte(0x82b6), restore_on_release: byte(0x82c1) != 0,
        }
    }
    fn default_levels(&mut self) -> [u16; 3] { Sources(&self.services.machine).class_gains(GainClass(5), 0) }
    fn curve(&mut self, inputs: &CurveInputs) -> Curve {
        let profile = inputs.profile;
        let temperature = inputs.offset as u16;
        let selected = gains::reference_gains(self.services, &GainRequest {
            mode: profile.adjustment.0, variant: profile.words[0].0, custom_source: profile.words[1].0,
            bank: profile.words[2].0,
            fixed: core::array::from_fn(|index| u16::from_le_bytes([profile.bytes[2 * index], profile.bytes[2 * index + 1]])),
            preset: 0, temperature, column: 0, level: 0, custom: inputs.averages,
            variant_set: 0, forced: true, variant_subset: 0,
        });
        Curve { levels: selected.gains, word: RetainedWord(selected.class.0) }
    }
    fn lens_resource(&mut self) -> ResourceIndex { ResourceIndex(self.services.machine.enter(0x76_d9c8, &[])) }
    fn resource_level(&mut self, resource: ResourceIndex) -> i16 { Exposure::relative_exposure(self.services, PipelineWord(resource.0)) }
    fn follow_limit(&mut self, selection: ProfileSelection, profile: RetainedWord, timing: TimingWord) -> u32 {
        rate_policy::policy_flag(&mut ProfileCalibration(self.geometry), selection == ProfileSelection::Record, profile.0, timing.0)
    }
    fn rate(&mut self, size: [PipelineWord; 2]) -> u32 { rate_policy::scaled_by_configured_factor(self.services, size[0].0.wrapping_mul(size[1].0)) }
    fn level_class(&mut self, level: i16, selection: ProfileSelection, window: RetainedWord) -> RetainedWord {
        RetainedWord(rate_policy::profile_curve(self.services, i32::from(level), selection == ProfileSelection::Record, window.0))
    }
    fn refresh_grid_selection(&mut self, record: RecordHandle) {
        let recognition = self.recognition;
        recognition.borrow_mut().vote(self, record, Mode::Live);
    }
    fn auxiliary_continues(&mut self, record: RecordHandle) -> bool { self.services.shared(record, &m::AT_0144) == 2 }
    fn auxiliary_profile(&mut self, record: RecordHandle) -> AuxiliaryProfile {
        let services = &mut self.services;
        let adjustment = if services.entries.header_byte(record, SharedHeaderByte::At63) != 1 { u32::from(services.entries.header_byte(record, SharedHeaderByte::At31)) } else { 0 };
        AuxiliaryProfile {
            adjustment: RetainedWord(adjustment),
            words: [u32::from(services.field(record, &r::AT_0048)), u32::from(services.field(record, &r::AT_002A)),
                u32::from(services.entries.header_byte(record, SharedHeaderByte::At46)), u32::from(services.field(record, &r::AT_003F)), u32::from(services.field(record, &r::AT_0036))].map(RetainedWord),
            bytes: services.field(record, &r::AT_000E),
        }
    }
    fn request_profile(&mut self, record: RecordHandle) -> RequestProfile {
        RequestProfile {
            profile: self.auxiliary_profile(record), resource: ResourceIndex(self.services.shared(record, &s::AT_0720)),
            word: RetainedWord(self.services.shared(record, &s::AT_0258)), halfword: RetainedHalfword(self.services.shared(record, &s::AT_0740)),
        }
    }
    fn follow_profile(&mut self, record: RecordHandle) -> RetainedWord { RetainedWord(u32::from(self.services.field(record, &r::AT_0034))) }
    fn curve_offset(&mut self, record: RecordHandle) -> i16 { self.services.shared(record, &s::AT_025C) }
    fn auxiliary_publication(&mut self, record: RecordHandle) -> AuxiliaryPublication {
        let bytes = self.services.shared_bytes(record, &a::AT_0180);
        let half = |at| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
        AuxiliaryPublication {
            curve: [half(0), half(2), half(4)], levels: [half(6), half(8), half(10)],
            published: u32::from_le_bytes(bytes[12..16].try_into().expect("auxiliary publication flag")) != 0,
            curve_word: RetainedWord(u32::from_le_bytes(bytes[16..20].try_into().expect("auxiliary curve word"))),
        }
    }
    fn publish_auxiliary(&mut self, record: RecordHandle, publication: AuxiliaryPublication) {
        let mut bytes = [0; 20];
        for (out, value) in bytes[..12].chunks_exact_mut(2).zip(publication.curve.into_iter().chain(publication.levels)) { out.copy_from_slice(&value.to_le_bytes()); }
        bytes[12..16].copy_from_slice(&u32::from(publication.published).to_le_bytes());
        bytes[16..20].copy_from_slice(&publication.curve_word.0.to_le_bytes());
        let _refused = self.services.records.write_shared(record, &a::AT_0180, bytes);
    }
    fn store_auxiliary_value(&mut self, record: RecordHandle, value: i16) { let _refused = self.services.records.write_shared(record, &s::AT_025C, value); }
    fn grid_selection(&mut self, record: RecordHandle) -> bool { self.services.entries.header_byte(record, SharedHeaderByte::At63) == 1 }
    fn record_capture_levels(&mut self, record: RecordHandle, levels: [u16; 3]) {
        if let Some(buffer) = self.services.shared(record, &m::CAPTURE_BUFFER) {
            for (index, value) in levels.into_iter().enumerate() { self.services.machine.write_u32(buffer.address() + 0x9e5e0 + 4 * index as u32, u32::from(value)); }
        }
    }
    fn rotate_adjustment_snapshot(&mut self, record: RecordHandle) { rate_policy::rotate_snapshot(self.services, record); }
}
