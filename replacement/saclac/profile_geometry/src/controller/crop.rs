#![forbid(unsafe_code)]

//! The primary operation's crop: the starter's crop rectangle aligned to the profile's
//! magnification and the format's block size, and its phase in the colour pattern.

use super::changes::{Mask, Work};
use super::{ProfileGeometry, Status, Tables};
use crate::memory::Memory;
use crate::ports::{ConfigOffset, Ports};
use crate::stock::layout::{controller::crop, work};

const LARGE_BLOCK_FORMAT: u32 = 1;
/// The format whose width needs no alignment.
const UNALIGNED_FORMAT: u32 = 2;
const CONFIGURED_EXPONENT: ConfigOffset = ConfigOffset(0x98cb);

/// The crop alignment modes that keep the starter's width.
const fn keeps_width(mode: u32) -> bool { matches!(mode, 0 | 4 | 5) }

const fn computes_phase(mode: u32) -> bool { mode >= 2 }

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// 0xee8300.
    pub(crate) fn prepare_crop(&mut self, mode: u32) -> Status {
        if !self.requested(Work::Crop) {
            return Ok(());
        }
        self.align_crop(mode);
        self.add_to_mask(Mask::Geometry, 0x1);
        self.clear_request(Work::Crop);
        Ok(())
    }

    /// The block height the format and scale mode give (0xee0ef0, 0xee0ec4).
    fn scale_denominator(&mut self, format: u32, scale_mode: u32) -> u32 {
        let exponent = match scale_mode {
            1 => self.ports.byte(CONFIGURED_EXPONENT) as u32,
            2 => 4,
            _ => 3,
        };
        let shift = (exponent + 3) as u8 as u32;
        if format == LARGE_BLOCK_FORMAT {
            ((((3u32 << (shift + 1)) as u16 as u32) << 1) / 3) as u16 as u32
        } else {
            (1u32 << shift) as u16 as u32
        }
    }

    /// 0xee71fc.
    fn align_crop(&mut self, mode: u32) {
        let format = self.memory.read_u32(crop::FORMAT);
        let denominator = self.scale_denominator(format, self.memory.read_u32(crop::SCALE_MODE));
        let profile = self.current_profile();
        let step = if format == UNALIGNED_FORMAT { 1 } else { self.profiles.scale_pair(profile).first as u16 };
        let magnification = self.profiles.magnification(profile);

        let top = self.memory.read_u32(crop::TOP);
        let excess = top % magnification;
        let mut height = self.memory.read_u32(crop::HEIGHT);
        if excess != 0 {
            self.memory.write_u32(crop::TOP, top - excess);
            height = height.wrapping_add(excess);
        }
        let height = height.wrapping_sub(1).wrapping_add(denominator) / denominator * denominator;
        self.memory.write_u32(crop::HEIGHT, height);

        let width = self.memory.read_u32(crop::WIDTH);
        if !keeps_width(mode) {
            let short = (width % step as u32) as u16;
            if short != 0 {
                self.memory.write_u32(crop::WIDTH, width.wrapping_add((step.wrapping_sub(short)) as u32));
            }
        }

        let top = self.memory.read_u32(crop::TOP);
        let left = self.memory.read_u32(crop::LEFT);
        let record = [top >> 1, height, left, self.memory.read_u32(crop::WIDTH)];
        for (index, value) in record.into_iter().enumerate() {
            self.memory.write_u16(work::GEOMETRY_RECORDS.plus(index as u16 * 2), value as u16);
        }

        if computes_phase(mode) {
            self.memory.write_u32(crop::TOP_EXCESS, excess);
            let attributes = self.profiles.class_of(profile).attributes;
            let pair = self.profiles.tables().default_pair();
            let first = ((top % pair.second + attributes[0] as u32) & 0xff) % pair.second;
            let second = ((left % pair.first + attributes[1] as u32) & 0xff) % pair.first;
            self.memory.write_u8(crop::PHASE[0], first as u8);
            self.memory.write_u8(crop::PHASE[1], second as u8);
        } else {
            self.memory.write_u32(crop::TOP_EXCESS, 0);
            self.memory.write_u8(crop::PHASE[0], 0);
            self.memory.write_u8(crop::PHASE[1], 0);
        }
    }
}
