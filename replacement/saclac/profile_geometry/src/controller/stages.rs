#![forbid(unsafe_code)]

//! The payload stages the refresh keeps current while a starter claims them: the two modes,
//! the tertiary stage, the quad selection and the format.

use super::changes::{Hold, Mask, Stage, Subscription, Work};
use super::{ProfileGeometry, Refusal, Status, Tables};
use crate::memory::Memory;
use crate::ports::{ConfigOffset, FormatPayload, ModeRequest, Ports};
use crate::profile::{PipelineGroup, ProfileHandle, QuadGroup, RectangleKind};
use crate::stock::layout::{changes, controller as field, parameters, work};

/// The profile the pipeline payload builders skip.
const UNBUILT_PROFILE: ProfileHandle = ProfileHandle(8);
/// Bit 2 of this configuration byte turns the quad selection off.
const QUAD_DISABLE: ConfigOffset = ConfigOffset(0xb2);
const QUAD_THRESHOLD: ConfigOffset = ConfigOffset(0x63a4);
const QUAD_BELOW_THRESHOLD: ConfigOffset = ConfigOffset(0x21a0);
const QUAD_AT_THRESHOLD: ConfigOffset = ConfigOffset(0x21a1);
const QUAD_LIMIT_OFFSET: ConfigOffset = ConfigOffset(0x1a7b);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum QuadSelection {
    Off,
    Gains,
    Area,
}

impl QuadSelection {
    const fn word(self) -> u32 {
        match self {
            QuadSelection::Off => 0,
            QuadSelection::Gains => 1,
            QuadSelection::Area => 2,
        }
    }
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    fn set_stage(&mut self, stage: Stage, active: bool) {
        if active {
            self.add_to_mask(Mask::Apply, stage.pipeline_bit());
            if !self.pending(stage) {
                self.enable(stage);
            }
        } else {
            self.finish_if_pending(stage);
        }
    }

    fn current_group(&self) -> PipelineGroup { self.profiles.pipeline_group(self.current_profile()) }

    /// 0xee4e08.
    pub(crate) fn update_modes(&mut self) {
        let group = if self.remainder_geometry() { PipelineGroup(1) } else { self.current_group() };
        if !self.requested(Work::Mode) {
            return;
        }
        if self.claimed(Stage::FirstMode) && self.claimed(Stage::SecondMode) {
            if self.held(Hold::Mode) || group == PipelineGroup::NONE {
                self.finish_if_pending(Stage::FirstMode);
                self.finish_if_pending(Stage::SecondMode);
            } else {
                let stages = if self.current_profile() == UNBUILT_PROFILE {
                    0
                } else {
                    let request = ModeRequest {
                        group,
                        index_scale: self.memory.read_u8(parameters::INDEX_SCALE),
                        index: self.memory.read_u8(parameters::INDEX),
                        shift: self.memory.read_i16(parameters::MODE_SHIFT),
                        mode: self.memory.read_u8(parameters::MODE_WORD),
                        first: parameters::MODE_FIRST,
                        second: parameters::MODE_SECOND,
                        mode_byte: self.memory.read_u8(parameters::MODE_BYTE) as i8,
                    };
                    self.ports.mode_payload(&request, work::MODE_PAYLOADS).0
                };
                let first = stages & Stage::FirstMode.pipeline_bit() != 0;
                self.set_stage(Stage::FirstMode, first);
                let second = stages & Stage::SecondMode.pipeline_bit() != 0;
                if second {
                    for at in work::SECOND_MODE_CLEARED {
                        self.memory.write_u32(at, 0);
                    }
                }
                self.set_stage(Stage::SecondMode, second);
            }
        }
        self.clear_request(Work::Mode);
    }

    /// 0xee6d08.
    pub(crate) fn update_tertiary(&mut self) {
        let group = self.current_group();
        if !self.requested(Work::Tertiary) {
            return;
        }
        if self.claimed(Stage::Tertiary) {
            if self.held(Hold::Tertiary) || group == PipelineGroup::NONE {
                self.finish_if_pending(Stage::Tertiary);
            } else {
                let filled = self.current_profile() != UNBUILT_PROFILE && {
                    let index_scale = self.memory.read_u8(parameters::INDEX_SCALE);
                    let index = self.memory.read_u8(parameters::INDEX);
                    let stages = self.ports.tertiary_payload(group, index_scale, index, work::TERTIARY_PAYLOAD);
                    stages.0 & Stage::Tertiary.pipeline_bit() != 0
                };
                if filled {
                    for at in work::TERTIARY_CLEARED {
                        self.memory.write_u32(at, 0);
                    }
                }
                self.set_stage(Stage::Tertiary, filled);
            }
        }
        self.clear_request(Work::Tertiary);
    }

    /// 0xee6414.
    pub(crate) fn select_quad(&mut self) {
        let group = self.profiles.quad_group(self.current_profile());
        if self.requested(Work::QuadSelection) {
            if self.claimed(Stage::QuadSelection) {
                let disabled = self.ports.byte(QUAD_DISABLE) & 4 != 0 || self.held(Hold::QuadSelection) || group == QuadGroup::NONE;
                if disabled {
                    if self.pending(Stage::QuadSelection) {
                        self.turn_quad_off();
                    }
                } else {
                    match self.quad_selection() {
                        1 => {
                            let limit = self.quad_limit();
                            let [first, second, third, fourth] = self.ports.quad_gains(group, limit);
                            for (at, value) in work::QUAD_GAINS.into_iter().zip([second, first, third, fourth]) {
                                self.memory.write_u16(at, value);
                            }
                            self.keep_quad(QuadSelection::Gains);
                        }
                        0 => self.turn_quad_off(),
                        2 => {
                            self.fill_quad_area();
                            self.keep_quad(QuadSelection::Area);
                        }
                        _ => {}
                    }
                }
            }
            self.clear_request(Work::QuadSelection);
        }
        let gains = self.quad_gains();
        self.set_diagnostic_gains(gains);
    }

    fn turn_quad_off(&mut self) {
        for at in work::QUAD_GAINS {
            self.memory.write_u16(at, 0);
        }
        self.memory.write_u32(work::QUAD_SELECTION, QuadSelection::Off.word());
        self.clear_quad_companion();
        self.finish(Stage::QuadSelection);
    }

    fn clear_quad_companion(&mut self) {
        let (at, bit) = changes::QUAD_SELECTION_CLEARED;
        self.memory.clear_bits_u8(at, bit);
    }

    fn keep_quad(&mut self, selection: QuadSelection) {
        self.clear_quad_companion();
        let current = self.memory.read_u32(work::QUAD_SELECTION) == selection.word();
        if !(self.pending(Stage::QuadSelection) && current) {
            self.memory.write_u32(work::QUAD_SELECTION, selection.word());
            self.enable(Stage::QuadSelection);
        }
        self.add_to_mask(Mask::Apply, Stage::QuadSelection.pipeline_bit());
    }

    /// 0xee631c.
    fn quad_limit(&mut self) -> i16 {
        let limit = self.memory.read_i16(parameters::SECONDARY_LIMIT);
        if self.profiles.entry(self.current_profile()).kind.0 > 1 {
            limit.wrapping_add(self.ports.signed_halfword(QUAD_LIMIT_OFFSET))
        } else {
            limit
        }
    }

    /// 0xee5794.
    fn quad_selection(&mut self) -> u8 {
        let profile = self.current_profile();
        if profile == UNBUILT_PROFILE {
            return 0;
        }
        let paired = matches!(profile.0, 0 | 8 | 9);
        let offset = if paired || self.profiles.entry(profile).kind.0 != 4 {
            match self.ports.quad_selection_offset(self.profiles.pipeline_group(profile)) {
                Some(offset) => offset,
                None => return 0,
            }
        } else {
            let level = self.memory.read_i16(parameters::SECONDARY_LIMIT) as i32 + self.profiles.aspect_value(profile) as i32;
            if level < self.ports.byte(QUAD_THRESHOLD) as i32 * 10 { QUAD_BELOW_THRESHOLD } else { QUAD_AT_THRESHOLD }
        };
        match self.ports.byte(offset) {
            selection @ (1 | 2) => selection,
            _ => 0,
        }
    }

    fn fill_quad_area(&mut self) {
        let profile = self.current_profile();
        let band = self.band(RectangleKind::Rows);
        let divisor = self.profiles.second_multiplier(profile);
        let height = (band.height as u32 * band.step as u32) & 0xfffe;
        let width = (band.width as u32 / divisor) & 0xfffe;
        let [top, left, height_at, width_at] = work::QUAD_AREA;
        self.memory.write_u16(top, band.top);
        self.memory.write_u16(left, (band.left as u32 / divisor) as u16);
        self.memory.write_u16(height_at, height as u16);
        self.memory.write_u16(width_at, width as u16);
        let scale = self.ports.quad_area_scale(width * height);
        self.memory.write_u32(work::QUAD_AREA_SCALE, scale);
        let limit = self.ports.quad_area_limit();
        self.memory.write_u16(work::QUAD_AREA_LIMIT, limit);
    }

    /// 0xee2d28.
    pub(crate) fn send_format(&mut self) -> Status {
        if !self.requested(Work::Format) {
            return Ok(());
        }
        if self.subscription_word(Subscription::Format) == 0 {
            self.clear_request(Work::Format);
            return Ok(());
        }
        let profile = self.current_profile();
        let second_scale = match self.profiles.second_pair_scale(profile) {
            0x10 => 4,
            0x14 => 8,
            scale => scale as u16,
        };
        let alignment = if self.remainder_geometry() { self.class_remainders(profile) } else { self.alignment(profile, true) };
        let payload = FormatPayload {
            first_scale: self.profiles.first_pair_scale(profile) as u16,
            second_scale,
            alignment,
            remainder_geometry: self.memory.read_u32(field::REMAINDER_GEOMETRY) != 0,
        };
        let accepted = self.ports.set_format(&payload);
        self.clear_request(Work::Format);
        if accepted { Ok(()) } else { Err(Refusal::PipelineRefused) }
    }
}
