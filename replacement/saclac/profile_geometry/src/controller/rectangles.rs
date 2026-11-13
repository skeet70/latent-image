#![forbid(unsafe_code)]

//! The profile's configured rectangles, normalized to its class, and the alignment of its
//! geometry to the sensor's colour pattern.

use super::{ProfileGeometry, Tables};
use crate::memory::Memory;
use crate::ports::{ConfigOffset, Ports};
use crate::profile::{ProfileHandle, Rectangle, RectangleKind};

/// The rectangle readers' band step.
const BAND_STEP: u16 = 16;

/// A rectangle whose rows or columns are read in bands of `step`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Band {
    pub top: u16,
    pub height: u16,
    pub left: u16,
    pub width: u16,
    pub step: u8,
}

impl Band {
    pub(crate) const ONES: Self = Self { top: 1, height: 1, left: 1, width: 1, step: 1 };
}

/// The even band count `size` holds and the offset that centres it.
const fn bands(size: u32) -> Option<(u16, u16)> {
    let count = (size >> 4) as u16 & 0xfffe;
    if count == 0 {
        return None;
    }
    let margin = ((size - ((count as u32) << 4)) >> 1) as u16;
    Some((count, margin))
}

impl<M: Memory, P: Ports, T: Tables> ProfileGeometry<M, P, T> {
    /// The configured `kind` rectangle (0xee7c98).
    fn configured_record(&mut self, profile: ProfileHandle, kind: RectangleKind) -> Rectangle {
        let Some(offsets) = self.profiles.rectangle_configuration(profile, kind) else {
            return Rectangle { top: 1, left: 1, width: 1, height: 1 };
        };
        let [top, left, width, height] =
            offsets.map(|offset| if offset == 0 { 1 } else { self.ports.halfword(ConfigOffset(offset)) as u32 });
        Rectangle { top, left, width, height }
    }

    /// Clips `record` to the profile's class, moves it to the configured origin and scales it
    /// to the first multiplier (0xedea20).
    fn normalize(&mut self, profile: ProfileHandle, mut record: Rectangle) -> Rectangle {
        let bounds = self.profiles.full_rectangle(profile);
        if bounds.height < record.height.wrapping_add(record.top) {
            record.top = 0;
            record.height = bounds.height;
        }
        if bounds.width < record.width.wrapping_add(record.left) {
            record.left = 0;
            record.width = bounds.width;
        }
        let origin = self.ports.configured_rectangle(profile);
        if record.top >= origin.top {
            record.top -= origin.top;
        }
        if record.left >= origin.left {
            record.left -= origin.left;
        }
        let scale = self.profiles.first_multiplier(profile);
        let (top, height) = (record.top, record.height);
        record.top = top / scale;
        record.height = (height / scale) & !1;
        record.width &= !1;
        if top % (scale << 1) != 0 && height != record.height {
            record.top += 1;
        }
        record
    }

    pub(crate) fn rectangle(&mut self, profile: ProfileHandle, kind: RectangleKind) -> Rectangle {
        let record = self.configured_record(profile, kind);
        self.normalize(profile, record)
    }

    /// 0xee0084, 0xee0104, 0xee01b0.
    pub(crate) fn band_of(&mut self, profile: ProfileHandle, kind: RectangleKind) -> Band {
        let record = self.rectangle(profile, kind);
        if record.height == 0 {
            return Band::ONES;
        }
        let plain = Band {
            top: record.top as u16,
            height: record.height as u16,
            left: record.left as u16,
            width: record.width as u16,
            step: 1,
        };
        match kind {
            RectangleKind::Plain => plain,
            RectangleKind::Rows => match bands(record.height) {
                Some((count, margin)) => {
                    Band { top: margin.wrapping_add(record.top as u16), height: count, step: BAND_STEP as u8, ..plain }
                }
                None => Band::ONES,
            },
            RectangleKind::Columns => match bands(record.width) {
                Some((count, margin)) => {
                    Band { left: margin.wrapping_add(record.left as u16), width: count, step: BAND_STEP as u8, ..plain }
                }
                None => Band::ONES,
            },
        }
    }

    pub(crate) fn band(&mut self, kind: RectangleKind) -> Band {
        let profile = self.current_profile();
        self.band_of(profile, kind)
    }

    /// The expanded rectangle's origin within the colour pattern (0xedcbe4).
    pub(crate) fn class_remainders(&mut self, profile: ProfileHandle) -> [u8; 2] {
        let class = self.profiles.class_of(profile);
        let expanded = self.ports.expanded_rectangle(profile);
        let pair = self.profiles.tables().default_pair();
        let first = (class.attributes[0] as u32).wrapping_add(expanded.top) % pair.second;
        let shifted = u32::from(class.second_flag == 2);
        let second = (class.attributes[1] as u32).wrapping_add(expanded.left).wrapping_add(shifted) % pair.first;
        [first as u8, second as u8]
    }

    /// The configured rectangle's origin within the colour pattern; `scaled` multiplies the
    /// origin by the pair scales (0xef5ea8 modes 0 and 1).
    pub(crate) fn alignment(&mut self, profile: ProfileHandle, scaled: bool) -> [u8; 2] {
        let class = self.profiles.class_of(profile);
        if !scaled {
            return class.attributes;
        }
        let first_shift = u32::from(class.first_flag == 2);
        let second_shift = u32::from(class.second_flag == 2);
        let first_scale = self.profiles.first_pair_scale(profile);
        let second_scale = self.profiles.second_pair_scale(profile);
        let origin = self.ports.configured_rectangle(profile);
        let pair = self.profiles.tables().default_pair();
        let [first_attribute, second_attribute] = class.attributes.map(u32::from);
        if pair.first == 2 && pair.second == 2 {
            let first = first_attribute.wrapping_add(origin.top).wrapping_add(first_shift) & 1;
            let second = second_attribute.wrapping_add(origin.left).wrapping_add(second_shift) & 1;
            return [first as u8, second as u8];
        }
        let first = origin.top.wrapping_mul(first_scale).wrapping_add(first_shift).wrapping_add(first_attribute) % pair.second;
        let second = origin.left.wrapping_mul(second_scale).wrapping_add(second_shift).wrapping_add(second_attribute) % pair.first;
        [first as u8, second as u8]
    }

    /// As `alignment(profile, true)` with the second phase advanced past the second scale
    /// (0xef5ea8 mode 2).
    pub fn advanced_alignment(&mut self, profile: ProfileHandle) -> [u8; 2] {
        let [first, second] = self.alignment(profile, true);
        let pair = self.profiles.tables().default_pair();
        if pair.first == 2 && pair.second == 2 {
            return [first, (second + 1) & 1];
        }
        let scale = self.profiles.second_pair_scale(profile);
        let whole = ((scale / pair.first) * pair.first + pair.first) & 0xffff;
        [first, ((second as u32 + whole).wrapping_sub(scale) % pair.first) as u8]
    }
}
