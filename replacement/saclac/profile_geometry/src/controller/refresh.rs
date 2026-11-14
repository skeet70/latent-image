#![forbid(unsafe_code)]

//! Taking a new parameter record or status snapshot: each changed field is stored and leaves the
//! work its consumers need for the operations refresh that follows.

use super::changes::{Stage, Subscription, Work};
use super::{Operation, ProfileGeometry, Status, Tables};
use crate::memory::{Block, Location, Memory};
use crate::numeric::round_tenths;
use crate::ports::{ConfigOffset, PipelineChannel, Ports};
use crate::profile::{ProfileHandle, ProfileKind};
use crate::stock::layout::{controller as field, counters, parameters, status, work};
use crate::stock::{Parameters, StatusSnapshot};

/// What a changed snapshot field leaves behind beside its new value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StatusEffect {
    Plain,
    /// The scan configuration is rebuilt when it is claimed.
    ScanConfiguration,
    /// As `ScanConfiguration`, and the diagnostic rate follows.
    Rate,
}

/// The triplet scales used for the status's output mode 1 on kind 3 profiles, by flag group,
/// and otherwise by class.
const SCALES_MODE_LOW: [u32; 3] = [0x82dc, 0x82db, 0x82dd];
const SCALES_MODE_MIDDLE: [u32; 3] = [0x82df, 0x82de, 0x82e0];
const SCALES_MODE_HIGH: [u32; 3] = [0x82d9, 0x82d8, 0x82da];
const SCALES_CLASS_ONE: [u32; 3] = SCALES_MODE_LOW;
const SCALES_OTHER: [u32; 3] = [0x82d6, 0x82d5, 0x82d7];

const CURVE_KNOTS: usize = 18;
const CURVE_FLOOR: i32 = 1000;
const CURVE_CEILING: i32 = 1850;
/// The factor a limit below the curves keeps.
const UNIT_FACTOR: i32 = 0x80;

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xed8af4.
    pub fn parameters(&self) -> Parameters {
        let mut bytes = [0; Parameters::BYTES];
        self.memory.read_bytes(Location::new(Block::Parameters, 0), &mut bytes);
        Parameters::from_bytes(bytes)
    }

    /// 0xed8b10.
    pub fn status_snapshot(&self) -> StatusSnapshot {
        let mut bytes = [0; StatusSnapshot::BYTES];
        self.memory.read_bytes(Location::new(Block::Status, 0), &mut bytes);
        StatusSnapshot::from_bytes(bytes)
    }

    /// 0xed8b2c.
    pub fn apply_parameters(&mut self, new: &Parameters) -> Status {
        self.update_parameters(new.bytes());
        let _ = self.refresh_operations();
        Ok(())
    }

    /// 0xed8b6c.
    pub fn apply_status(&mut self, new: &StatusSnapshot) -> Status {
        self.update_status(new.bytes());
        let _ = self.refresh_operations();
        Ok(())
    }

    /// Stores `new` at `at` when it differs; true when it did.
    fn replace(&mut self, at: Location, new: &[u8]) -> bool {
        let changed = new.iter().enumerate().any(|(index, &byte)| self.memory.read_u8(at.plus(index as u16)) != byte);
        if changed {
            self.memory.write_bytes(at, new);
        }
        changed
    }

    fn replace_from(&mut self, at: Location, record: &[u8], bytes: usize) -> bool {
        let offset = at.offset as usize;
        self.replace(at, &record[offset..offset + bytes])
    }

    pub(crate) fn current_profile(&self) -> ProfileHandle { ProfileHandle(self.memory.read_u32(parameters::PROFILE)) }

    fn request_if(&mut self, condition: bool, work: Work) {
        if condition {
            self.request(work);
        }
    }

    fn request_mode_if_claimed(&mut self) {
        let claimed = self.claimed(Stage::FirstMode) || self.claimed(Stage::SecondMode);
        self.request_if(claimed, Work::Mode);
    }

    fn request_scan_configuration_if_claimed(&mut self) {
        let claimed = self.scan_claimed();
        self.request_if(claimed, Work::ScanConfiguration);
    }

    fn request_scans_if_claimed(&mut self) {
        let primary = self.subscribed(Subscription::PrimaryScan) && self.claimed(Stage::PrimaryScan);
        self.request_if(primary, Work::PrimaryScan);
        let secondary = self.subscribed(Subscription::SecondaryScan) && self.claimed(Stage::SecondaryScan);
        self.request_if(secondary, Work::SecondaryScan);
    }

    fn scan_running(&self) -> bool {
        self.operation(Operation::First).running() || self.operation(Operation::Second).running()
    }

    fn request_record_rectangles(&mut self, with_set: bool) {
        if self.operation(Operation::Third).running() {
            self.request(Work::ExtendedRectangles);
            self.request(Work::CompactRectangles);
            self.request(Work::RecordRectangles);
            self.request_if(with_set, Work::RectangleSet);
        }
        let indexed = self.operation(Operation::Indexed).running();
        self.request_if(indexed, Work::IndexedPair);
    }

    fn request_rectangles(&mut self, with_set: bool) {
        if self.scan_running() {
            let primary = self.subscribed(Subscription::PrimaryScan);
            self.request_if(primary, Work::PrimaryRectangles);
            let secondary = self.subscribed(Subscription::SecondaryScan);
            self.request_if(secondary, Work::SecondaryRectangles);
        }
        self.request_record_rectangles(with_set);
    }

    /// 0xee1bc0.
    fn update_parameters(&mut self, new: &[u8; Parameters::BYTES]) {
        let word = |at: Location| u32::from_le_bytes(core::array::from_fn(|index| new[at.offset as usize + index]));
        let half = |at: Location| i16::from_le_bytes([new[at.offset as usize], new[at.offset as usize + 1]]);
        self.update_profile(ProfileHandle(word(parameters::PROFILE)));

        let limit = half(parameters::LIMIT);
        if self.replace_from(parameters::LIMIT, new, 2) {
            self.set_diagnostic_limit(limit);
            let mut triplet_bytes = [0; 6];
            self.memory.read_bytes(status::TRIPLET, &mut triplet_bytes);
            let triplet = core::array::from_fn(|index| u16::from_le_bytes([triplet_bytes[index * 2], triplet_bytes[index * 2 + 1]]));
            let adjusted = self.adjust_triplet(triplet, limit);
            if adjusted != self.white_balance() {
                for (at, value) in work::WHITE_BALANCE.into_iter().zip(adjusted) {
                    self.memory.write_u16(at, value);
                }
                self.request_scan_configuration_if_claimed();
                self.set_diagnostic_white_balance(adjusted);
            }
        }

        let secondary = half(parameters::SECONDARY_LIMIT);
        if self.replace_from(parameters::SECONDARY_LIMIT, new, 2) {
            if self.subscribed(Subscription::Format) {
                self.request(Work::QuadSelection);
                self.request(Work::SecondaryPayload);
            }
            self.request_scan_configuration_if_claimed();
            self.set_diagnostic_secondary_limit(secondary);
        }

        let index = self.ports.collapse_index(word(parameters::INDEX));
        if self.replace(parameters::INDEX, &index.to_le_bytes()) {
            self.request_mode_if_claimed();
            let tertiary = self.claimed(Stage::Tertiary);
            self.request_if(tertiary, Work::Tertiary);
            let scale = self.memory.read_u32(parameters::INDEX_SCALE);
            self.set_diagnostic_index(scale, index);
        }

        let scale = word(parameters::INDEX_SCALE);
        if self.replace_from(parameters::INDEX_SCALE, new, 4) {
            self.request_mode_if_claimed();
            let tertiary = self.claimed(Stage::Tertiary);
            self.request_if(tertiary, Work::Tertiary);
            self.request_scan_configuration_if_claimed();
            let index = self.memory.read_u32(parameters::INDEX);
            self.set_diagnostic_index(scale, index);
        }

        if self.replace_from(parameters::AREA, new, 12) {
            self.request_rectangles(false);
            self.set_diagnostic_value(self.memory.read_u32(parameters::AREA));
        }
        if self.replace_from(parameters::FIRST_DIVISOR, new, 4) {
            self.request_record_rectangles(false);
        }
        self.replace_from(parameters::QUAD_MODE, new, 4);
        self.replace_from(parameters::SCAN_FIRST, new, 2);
        self.replace_from(parameters::SCAN_SECOND, new, 2);
        if self.profiles.is_extended_class(ProfileHandle(word(parameters::PROFILE))) {
            self.replace_from(parameters::EXTENDED_PAIR, new, 8);
        } else {
            self.memory.fill(parameters::EXTENDED_PAIR, 8, 0);
        }
        self.replace_from(parameters::MODE_SHIFT, new, 2);
        if self.replace_from(parameters::MODE_WORD, new, 4) {
            self.request_mode_if_claimed();
            self.request_scans_if_claimed();
        }
        for (at, bytes) in [(parameters::MODE_FIRST, 4), (parameters::MODE_SECOND, 4), (parameters::MODE_BYTE, 1)] {
            if self.replace_from(at, new, bytes) {
                self.request_mode_if_claimed();
            }
        }
    }

    /// 0xee111c.
    fn update_profile(&mut self, profile: ProfileHandle) {
        if self.replace(parameters::PROFILE, &profile.0.to_le_bytes()) {
            if self.subscribed(Subscription::Output) {
                self.request(Work::Output);
                self.request(Work::OutputCompanion);
                self.request(Work::Format);
            }
            if self.subscribed(Subscription::Format) {
                self.request(Work::PendingPair);
                self.request(Work::FormatCompanion);
                self.request(Work::SecondaryPayload);
                self.request(Work::PrimaryPayload);
            }
            let fourth = self.subscribed(Subscription::Fourth);
            self.request_if(fourth, Work::FourthCompanion);
            let primary = self.operation(Operation::Primary).running();
            self.request_if(primary, Work::Crop);
            self.request_rectangles(true);
            self.request_mode_if_claimed();
            let quad = self.claimed(Stage::QuadSelection);
            self.request_if(quad, Work::QuadSelection);
            let tertiary = self.claimed(Stage::Tertiary);
            self.request_if(tertiary, Work::Tertiary);
            let channel = self.channel_for(profile);
            self.memory.write_u32(field::PIPELINE_CHANNEL, channel.0);
        }
        self.describe_profile(self.current_profile());
    }

    fn channel_for(&self, profile: ProfileHandle) -> PipelineChannel {
        if profile.0.wrapping_sub(8) <= 1 {
            return PipelineChannel(1);
        }
        if profile.0 == 0 {
            return PipelineChannel(2);
        }
        let single_flag = self.profiles.flags_low(profile) == 1;
        PipelineChannel(match self.profiles.entry(profile).kind.0 {
            0 => 1,
            1 if single_flag => 5,
            1 => 1,
            2 => 2,
            3 if single_flag => 5,
            3 => 2,
            4 | 5 => 0,
            _ => 6,
        })
    }

    /// 0xee2adc.
    fn update_status(&mut self, new: &[u8; StatusSnapshot::BYTES]) {
        let word = |offset: u16| u32::from_le_bytes(core::array::from_fn(|index| new[offset as usize + index]));
        if self.replace_from(status::AREA, new, 12) {
            self.request_rectangles(false);
            self.request_scan_configuration_if_claimed();
        }
        for (offset, bytes, effect) in status::FIELDS {
            if self.replace_from(Location::new(Block::Status, offset), new, bytes as usize) {
                match effect {
                    StatusEffect::Plain => {}
                    StatusEffect::ScanConfiguration => self.request_scan_configuration_if_claimed(),
                    StatusEffect::Rate => {
                        self.request_scan_configuration_if_claimed();
                        self.set_diagnostic_rate(word(offset) & 0xffff);
                    }
                }
            }
        }
        if self.replace_from(status::SCAN_PAIR, new, 8) {
            self.request_scan_configuration_if_claimed();
        }
        for (at, subscription, work) in [
            (status::PRIMARY_PAIR, Subscription::PrimaryScan, Work::PrimaryRectangles),
            (status::SECONDARY_PAIR, Subscription::SecondaryScan, Work::SecondaryRectangles),
        ] {
            if self.replace_from(at, new, 8) {
                let wanted = self.scan_running() && self.subscribed(subscription);
                self.request_if(wanted, work);
            }
        }
        if self.replace_from(status::TRIPLET, new, 6) {
            let offset = status::TRIPLET.offset as usize;
            let triplet = core::array::from_fn(|index| u16::from_le_bytes([new[offset + index * 2], new[offset + index * 2 + 1]]));
            let adjusted = self.adjust_triplet(triplet, self.memory.read_i16(parameters::LIMIT));
            for (at, value) in work::WHITE_BALANCE.into_iter().zip(adjusted) {
                self.memory.write_u16(at, value);
            }
            self.request_scan_configuration_if_claimed();
            self.set_diagnostic_white_balance(adjusted);
        }
        self.track_index(word(status::TRACKING_ARMED.offset), word(status::TRACKING_HELD.offset), word(status::TRACKED_INDEX.offset), word(status::SCAN_PROFILE.offset));
    }

    /// 0xee2524.
    fn track_index(&mut self, armed: u32, held: u32, index: u32, scan_profile: u32) {
        self.memory.write_u32(status::TRACKING_ARMED, armed);
        self.memory.write_u32(status::TRACKING_HELD, held);
        if armed == 1 && held == 0 {
            let mut changed = false;
            if self.replace(status::TRACKED_INDEX, &index.to_le_bytes()) && index != self.memory.read_u32(counters::INDEX) {
                if self.memory.read_u32(counters::TRACKING) == 0 {
                    self.memory.write_u32(counters::INDEX, index);
                }
                changed = true;
            }
            changed |= self.replace(status::SCAN_PROFILE, &scan_profile.to_le_bytes());
            let lagging = self.memory.read_u32(counters::TRACKING) == 1
                && self.memory.read_u32(counters::INDEX) != self.memory.read_u32(status::TRACKED_INDEX);
            if lagging || changed {
                self.request_scans_if_claimed();
            }
        }
        self.memory.write_u32(status::TRACKING_ARMED, 0);
    }

    /// Scales the status triplet for the current profile and `limit` (0xee5ec0).
    pub(crate) fn adjust_triplet(&mut self, triplet: [u16; 3], limit: i16) -> [u16; 3] {
        let profile = self.current_profile();
        let entry = self.profiles.entry(profile);
        let curve = match entry.kind.0 {
            2 if self.profiles.flags_low(profile) == 0 => match (entry.class.0, self.profiles.flags_high(profile)) {
                (1, _) => 3,
                (_, 2) => 1,
                (_, 3) => 3,
                (_, 0) => 2,
                _ => 0,
            },
            2 => match self.profiles.flags_high(profile) {
                2 => 6,
                3 => 3,
                _ => 5,
            },
            3 => 3,
            4 | 5 => 4,
            _ => return self.ports.limit_triplet(triplet),
        };
        let scale = |value: u16, factor: u32| ((value as u32).wrapping_mul(factor) >> 7) as u16;
        let [mut first, mut second, mut third] = triplet;
        first = scale(first, self.profile_curve(limit as i32, curve) as u32);
        let [second_scale, third_scale] = self.feature_scales(profile);
        second = scale(second, second_scale as u32);
        third = scale(third, third_scale as u32);
        let offsets = if self.memory.read_u32(status::OUTPUT_MODE) == 1 && entry.kind == ProfileKind(3) {
            match self.profiles.flags_low(profile) {
                0 | 1 => SCALES_MODE_LOW,
                3..=5 => SCALES_MODE_MIDDLE,
                _ => SCALES_MODE_HIGH,
            }
        } else if entry.class.0 == 1 {
            SCALES_CLASS_ONE
        } else {
            SCALES_OTHER
        };
        let [first_offset, second_offset, third_offset] = offsets.map(|offset| self.ports.byte(ConfigOffset(offset)) as u32);
        let adjusted = [scale(first, first_offset), scale(second, second_offset), scale(third, third_offset)];
        self.ports.limit_triplet(adjusted)
    }

    /// The factor in 128ths the triplet's first value takes at `limit` (0xef3dac).
    pub(crate) fn profile_curve(&mut self, limit: i32, curve: u8) -> i32 {
        if limit < CURVE_FLOOR {
            return UNIT_FACTOR;
        }
        let limit = limit.min(CURVE_CEILING) as i16;
        let base = match curve {
            1 | 4 | 6 => 0x221c,
            2 => 0x2240,
            3 => 0x21b0,
            5 => 0x21f8,
            _ => 0x21d4,
        };
        let mut point = |index: usize| self.ports.halfword(ConfigOffset(base + index as u32 * 2));
        if limit == CURVE_FLOOR as i16 {
            return point(0) as i32;
        }
        let tables = self.profiles.tables();
        let Some(upper) = (1..CURVE_KNOTS).find(|&index| tables.curve_knot(index) >= limit) else {
            return point(CURVE_KNOTS) as i32;
        };
        let (lower_knot, upper_knot) = (tables.curve_knot(upper - 1), tables.curve_knot(upper));
        if upper_knot == limit {
            return point(upper) as i32;
        }
        let low = (point(upper - 1) as u32 * 10) as u16;
        let high = point(upper) as u32 * 10;
        let rise = (low as u32).wrapping_sub(high) as i16 as i32;
        let run = lower_knot.wrapping_sub(upper_knot) as i32;
        let at_limit = (limit as i32 * rise).checked_div(run).unwrap_or(0);
        let at_lower = (lower_knot as i32 * rise).checked_div(run).unwrap_or(0);
        round_tenths(at_limit + low as i16 as i32 - at_lower)
    }

    /// The profile feature's scales for the triplet's second and third values (0xef643c).
    pub(crate) fn feature_scales(&mut self, profile: ProfileHandle) -> [u8; 2] {
        let entry = self.profiles.entry(profile);
        if entry.kind == ProfileKind(1) {
            return [UNIT_FACTOR as u8; 2];
        }
        let column = match (entry.kind.0, profile.0) {
            (0, _) => 2,
            (_, 10) => 1,
            _ => 0,
        };
        let offsets = self.profiles.tables().feature_scale_offsets(entry.feature as u32 * 3 + column);
        offsets.map(|offset| if offset == 0 { UNIT_FACTOR as u8 } else { self.ports.byte(ConfigOffset(offset)) })
    }
}
