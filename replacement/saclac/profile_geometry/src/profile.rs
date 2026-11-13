#![forbid(unsafe_code)]

//! The image's DCF profile table: each profile names a geometry class, a kind, flags and
//! indices into the frame-rate threshold and aspect tables.

use crate::numeric::runtime_quotient;

/// An index into the profile table; nothing bounds it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileHandle(pub u32);

/// An index into the geometry class table, read from a profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClassIndex(pub u32);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileKind(pub u8);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileEntry {
    pub class: ClassIndex,
    pub kind: ProfileKind,
    pub flags: u16,
    pub threshold_index: u8,
    pub feature: u8,
    pub aspect_index: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GeometryClass {
    pub height: u16,
    pub width: u16,
    pub top: u16,
    pub left: u16,
    pub crop_height: u16,
    pub crop_width: u16,
    pub first_scale: u8,
    pub second_scale: u8,
    pub first_flag: u8,
    pub second_flag: u8,
    pub scale_selection: u8,
    pub attributes: [u8; 2],
    pub kind: u8,
    pub magnification_index: u8,
    pub limit: u8,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rectangle {
    pub top: u32,
    pub left: u32,
    pub width: u32,
    pub height: u32,
}

/// The two scale divisors every class shares (0xef5768); (2, 2) disables per-class scaling.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScalePair {
    pub first: u32,
    pub second: u32,
}

/// The table template's expansion ratio, `numerator * 4 / divisor` (0xef5658).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExpansionRatio {
    pub numerator: u8,
    pub divisor: u8,
}

/// A profile's pipeline group (0xed7db0); 14 outside the row table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PipelineGroup(pub u32);

impl PipelineGroup {
    /// The group with no mode or tertiary payloads.
    pub const NONE: Self = Self(14);
}

/// A profile's quad selection group (0xed7d84); 3 outside the row table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QuadGroup(pub u32);

impl QuadGroup {
    /// The group with no quad selection.
    pub const NONE: Self = Self(3);
}

/// A profile's row in the image's pipeline table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProfileRow {
    /// Selects the rectangle configuration row; 16 and 17 have none (0xed7d64).
    pub rectangle_row: u32,
    pub pipeline_group: PipelineGroup,
    /// 0xed7ddc; 1 outside the table.
    pub policy: u32,
    pub quad_group: QuadGroup,
}

/// The three rectangle configurations a profile's rectangle row selects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RectangleKind {
    Plain,
    Rows,
    Columns,
}

pub trait ProfileTables {
    fn entry(&self, profile: ProfileHandle) -> ProfileEntry;
    fn class(&self, class: ClassIndex) -> GeometryClass;
    /// `index` is below 22.
    fn threshold(&self, index: u8) -> u16;
    fn default_pair(&self) -> ScalePair;
    fn expansion(&self) -> ExpansionRatio;
    /// The limits at which the triplet curves are tabulated, rising; `index` is below 18.
    fn curve_knot(&self, index: usize) -> i16;
    /// The configuration offsets of a feature's two triplet scales, zero for none; `row` is
    /// three per feature plus the profile's column.
    fn feature_scale_offsets(&self, row: u32) -> [u32; 2];
    /// `index` is below 36.
    fn row(&self, index: u32) -> ProfileRow;
    /// `index` is below 2.
    fn aspect_value(&self, index: u8) -> i16;
    /// The configuration offsets of a rectangle's four values, zero for 1; `row` is below 16.
    fn rectangle_configuration(&self, kind: RectangleKind, row: u32) -> [u32; 4];
    /// `slot` is below 10.
    fn scan_slot(&self, slot: u32) -> ScanSlot;
    /// A status code and its pipeline code; `index` is below 62.
    fn record_code(&self, index: u32) -> [u16; 2];
}

/// A row of the image's scan slot table, which a profile's kind and class select.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScanSlot {
    pub margin: u16,
    pub base: u16,
    pub divisor: u16,
    pub height: u16,
    pub width: u16,
}

const PROFILE_ROWS: u32 = 36;

const THRESHOLDS: u8 = 22;

#[derive(Clone, Debug)]
pub struct Profiles<T> {
    tables: T,
}

impl<T: ProfileTables> Profiles<T> {
    pub const fn new(tables: T) -> Self { Self { tables } }

    pub fn tables(&self) -> &T { &self.tables }

    pub fn entry(&self, profile: ProfileHandle) -> ProfileEntry { self.tables.entry(profile) }

    pub fn class_of(&self, profile: ProfileHandle) -> GeometryClass { self.tables.class(self.entry(profile).class) }

    /// 0xef5334.
    pub fn threshold_value(&self, index: u8) -> u16 {
        self.tables.threshold(if index < THRESHOLDS { index } else { 0 })
    }

    /// 0xef5360.
    pub fn selected_threshold(&self, profile: ProfileHandle) -> u16 { self.threshold_value(self.entry(profile).threshold_index) }

    /// 0xef5384.
    pub fn threshold_index_for(&self, value: u32) -> u8 {
        if value >= 400 {
            return THRESHOLDS - 1;
        }
        (2..=20).rev().find(|&index| self.tables.threshold(index) as u32 <= value).unwrap_or(1)
    }

    /// 0xef53f8.
    pub fn aspect_index(&self, profile: ProfileHandle) -> u8 { self.entry(profile).aspect_index }

    /// 0xef5420.
    pub fn threshold_index(&self, profile: ProfileHandle) -> u8 { self.entry(profile).threshold_index }

    /// 0xef5138.
    pub fn full_rectangle(&self, profile: ProfileHandle) -> Rectangle {
        let class = self.class_of(profile);
        Rectangle { top: 0, left: 0, width: class.width as u32, height: class.height as u32 }
    }

    /// 0xef5470.
    pub fn crop_rectangle(&self, profile: ProfileHandle, preserve_axes: bool) -> Rectangle {
        let entry = self.entry(profile);
        let class = self.tables.class(entry.class);
        let crop = Rectangle {
            top: class.top as u32,
            left: class.left as u32,
            width: class.crop_width as u32,
            height: class.crop_height as u32,
        };
        if preserve_axes || entry.kind != ProfileKind(0) {
            return crop;
        }
        Rectangle { top: crop.top >> 1, left: crop.left << 1, width: crop.width << 1, height: (crop.height >> 1) + 1 }
    }

    /// 0xef5658.
    pub fn expanded_ratio(&self, value: u32) -> u64 {
        let ratio = self.tables.expansion();
        runtime_quotient(value as u64 * (ratio.numerator as u64 * 4), ratio.divisor as u64)
    }

    /// 0xef6884.
    pub fn is_extended_class(&self, profile: ProfileHandle) -> bool { (9..=15).contains(&self.entry(profile).class.0) }

    /// 0xef592c.
    pub fn requires_adjustment(&self, profile: ProfileHandle) -> bool {
        match self.entry(profile).kind.0 {
            0 | 1 | 4 => false,
            5 => self.flag0(profile) && !self.is_extended_class(profile),
            _ => true,
        }
    }

    /// 0xef57bc, 0xef57e8, 0xef5840.
    pub fn flags_low(&self, profile: ProfileHandle) -> u8 { (self.entry(profile).flags & 0xf) as u8 }

    /// 0xef5814.
    pub fn flags_high(&self, profile: ProfileHandle) -> u8 { ((self.entry(profile).flags >> 4) & 0xf) as u8 }

    /// 0xef586c.
    pub fn flag0(&self, profile: ProfileHandle) -> bool { self.entry(profile).flags & 1 != 0 }

    /// 0xef663c.
    pub fn flag14(&self, profile: ProfileHandle) -> bool { self.entry(profile).flags & (1 << 14) != 0 }

    /// 0xef58c0.
    pub fn class_kind(&self, profile: ProfileHandle) -> u8 { self.class_of(profile).kind }

    /// 0xef5240.
    pub fn class_limit(&self, profile: ProfileHandle) -> u8 { self.class_of(profile).limit }

    /// 0xef599c.
    pub fn first_scale(&self, profile: ProfileHandle) -> u8 { self.class_of(profile).first_scale }

    /// 0xef59c0.
    pub fn second_scale(&self, profile: ProfileHandle) -> u8 { self.class_of(profile).second_scale }

    /// 0xef52b8.
    pub fn selection_multiplier(&self, profile: ProfileHandle) -> u32 {
        match self.class_of(profile).scale_selection {
            1 => 2,
            2 => 4,
            _ => 1,
        }
    }

    /// 0xef5a8c.
    pub fn first_multiplier(&self, profile: ProfileHandle) -> u32 {
        if self.class_of(profile).first_flag != 0 { 2 } else { self.selection_multiplier(profile) }
    }

    /// 0xef5ca8.
    pub fn second_multiplier(&self, profile: ProfileHandle) -> u32 {
        if self.class_of(profile).second_flag != 0 { 2 } else { 1 }
    }

    pub fn row(&self, profile: ProfileHandle) -> Option<ProfileRow> {
        (profile.0 < PROFILE_ROWS).then(|| self.tables.row(profile.0))
    }

    /// 0xed7db0.
    pub fn pipeline_group(&self, profile: ProfileHandle) -> PipelineGroup {
        self.row(profile).map_or(PipelineGroup::NONE, |row| row.pipeline_group)
    }

    /// 0xed7d84.
    pub fn quad_group(&self, profile: ProfileHandle) -> QuadGroup {
        self.row(profile).map_or(QuadGroup::NONE, |row| row.quad_group)
    }

    /// 0xed7ddc.
    pub fn policy(&self, profile: ProfileHandle) -> u32 { self.row(profile).map_or(1, |row| row.policy) }

    /// The configuration offsets of the profile's `kind` rectangle, or `None` for a profile whose
    /// rectangles are all ones (0xee7c98).
    pub fn rectangle_configuration(&self, profile: ProfileHandle, kind: RectangleKind) -> Option<[u32; 4]> {
        let row = self.row(profile)?.rectangle_row;
        (row < 16).then(|| self.tables.rectangle_configuration(kind, row))
    }

    /// 0xef5310.
    pub fn aspect_value(&self, profile: ProfileHandle) -> i16 {
        let index = self.aspect_index(profile);
        self.tables.aspect_value(if index > 1 { 0 } else { index })
    }

    /// 0xef5ab8.
    pub fn scaled_first(&self, profile: ProfileHandle, multiplied: bool) -> u16 {
        let scale = self.first_scale(profile) as u32;
        (if multiplied { scale * self.first_multiplier(profile) } else { scale }) as u16
    }

    /// 0xef5cc4.
    pub fn scaled_second(&self, profile: ProfileHandle, multiplied: bool) -> u16 {
        let scale = self.second_scale(profile) as u32;
        (if multiplied { scale * self.second_multiplier(profile) } else { scale }) as u16
    }

    fn scaling_disabled(&self) -> Option<ScalePair> {
        let pair = self.tables.default_pair();
        (pair.first == 2 && pair.second == 2).then_some(pair)
    }

    /// 0xef5af0.
    pub fn first_pair_scale(&self, profile: ProfileHandle) -> u32 {
        match self.scaling_disabled() {
            Some(_) => 1,
            None => self.scaled_first(profile, true) as u32,
        }
    }

    /// 0xef5cfc.
    pub fn second_pair_scale(&self, profile: ProfileHandle) -> u32 {
        match self.scaling_disabled() {
            Some(_) => 1,
            None => self.scaled_second(profile, true) as u32,
        }
    }

    /// 0xef59e4.
    pub fn scale_pair(&self, profile: ProfileHandle) -> ScalePair {
        if let Some(pair) = self.scaling_disabled() {
            return pair;
        }
        let class = self.class_of(profile);
        self.halved(class.first_scale != 1, class.second_scale != 1)
    }

    /// 0xef5b5c.
    pub fn flagged_scale_pair(&self, profile: ProfileHandle) -> ScalePair {
        if let Some(pair) = self.scaling_disabled() {
            return pair;
        }
        let class = self.class_of(profile);
        self.halved(class.first_flag != 0 || class.first_scale != 1, class.second_flag != 0 || class.second_scale != 1)
    }

    fn halved(&self, second: bool, first: bool) -> ScalePair {
        let pair = self.tables.default_pair();
        ScalePair {
            first: if first { pair.first >> 1 } else { pair.first },
            second: if second { pair.second >> 1 } else { pair.second },
        }
    }

    /// 0xef653c.
    pub fn magnification_index(&self, profile: ProfileHandle) -> u8 { self.class_of(profile).magnification_index }

    /// 0xef6560.
    pub fn magnification(&self, profile: ProfileHandle) -> u32 {
        match self.magnification_index(profile) {
            0 => 1,
            2 => 4,
            3 => 6,
            4 => 8,
            5 => 10,
            6 => 12,
            _ => 2,
        }
    }
}

/// 0xef4fd8.
pub const fn normalize_selector(selector: u32) -> u32 {
    match selector {
        5..=10 => selector,
        _ => 0,
    }
}

/// 0xef7820.
pub const fn is_paired_mode(mode: u32) -> bool { matches!(mode, 3 | 4) }
