#![forbid(unsafe_code)]

//! The layers the shot's selection grid calls: image-4 profile values, the calculation library's
//! selection functions, the image layers' class and cell codes, and the configuration.

use crate::calculation_work::CalculationProfile;
use xm1_saclac_interface::selection::{CellCode, SelectionClass, SubjectQuery};
use xm1_saclac_interface::RecordHandle;

/// Upper and lower class codes a selection may take.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClassLimits {
    pub upper: u8,
    pub lower: u8,
}

impl ClassLimits {
    /// The part of these limits inside `other`; disjoint limits collapse onto the nearer bound of
    /// these (0xf1aa70).
    pub fn within(self, other: Self) -> Self {
        if self.upper < other.lower {
            Self { upper: self.upper, lower: self.upper }
        } else if self.lower > other.upper {
            Self { upper: self.lower, lower: self.lower }
        } else {
            Self { upper: self.upper.min(other.upper), lower: self.lower.max(other.lower) }
        }
    }
}

/// The record values the calculation library derives a record's class limits from (0xf5f6c0).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LimitInputs {
    pub mode: u32,
    pub flags: u16,
    pub code: i16,
    pub at0045: u32,
    pub adjustment_option: u32,
    pub adjustment_kind: u32,
    pub at003e: u32,
}

/// The values the calculation library computes a tracked kind's target level from (0xf60eb4).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LevelQuery {
    pub mode: u32,
    pub flags: u16,
    pub at004b: u32,
    pub at0066: u32,
    pub pass: u32,
    pub at0045: u32,
    pub adjustment_option: u32,
    pub kind: u32,
    pub at003e: u32,
    pub at0024: i16,
}

/// The values the calculation library computes the grid's target reference from (0xf6024c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReferenceQuery {
    pub record: RecordHandle,
    pub target_word: u32,
    pub focus_mode: u32,
    pub cell: CellCode,
    pub lens_reference: u32,
    pub refinement: u32,
}

/// The values the image layer decides an area's class change from (0xef6778, 0xef67d8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ClassQuery {
    pub total: i16,
    pub mode: u32,
    pub flags: u16,
    pub at0032: u32,
    pub at0033: u8,
    pub at0026: u16,
    pub adjustment_kind: u32,
    pub at0034: u32,
    pub resume_lead: i16,
    pub cell_class: u32,
    pub row_flagged: bool,
}

/// The record values the image layer takes a list's default class from (0xfa67a8).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DefaultQuery {
    pub code: i16,
    pub selection_kind: u32,
    pub mode: u32,
    pub at0032: u32,
    pub at0033: u8,
    pub at0026: u16,
    pub adjustment_kind: u32,
    pub row_limit: u32,
}

/// The values the calculation library blends a focus reference from (0xf4d700).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BlendQuery {
    pub mode: u32,
    pub pass: u32,
    pub at0045: u32,
    pub adjustment_option: u32,
    pub at003e: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridSetting {
    /// Bit 0 takes one fixed class for every area instead of choosing classes.
    Control,
    /// Bit 0 caps the subject kinds of a mode-3 classification.
    KindCap,
    /// 100, 200 or 300 names the kind the cap allows.
    KindCapClass,
    /// The tenths a tracked kind's level may reach before its target gate closes.
    TargetGate,
    /// The three thresholds that sort a scaled level into a kind, per total band.
    KindThreshold { band: u8, step: u8 },
    /// The threshold below which a subject keeps its kind, per total band.
    EligibilityThreshold { band: u8 },
    /// A focus margin in tenths, per margin entry and level step.
    FocusMargin { entry: u8, step: u8 },
}

/// Image-4 profile values (the calculation step's accessors).
pub trait GridProfiles {
    fn profile(&mut self, record: RecordHandle, value: CalculationProfile) -> u32;
}

/// The calculation library's selection functions.
pub trait GridLibrary {
    fn selection_mode(&mut self, record: RecordHandle, refinement: u32) -> u32;
    fn selection_flags(&mut self, record: RecordHandle, refinement: u32) -> u16;
    /// Nonzero bounds a chosen class by the record's limits instead of searching for one.
    fn selection_code(&mut self, record: RecordHandle) -> i16;
    fn class_limits(&mut self, inputs: &LimitInputs) -> ClassLimits;
    fn fixed_class_limits(&mut self) -> ClassLimits;
    fn total_class_limits(&mut self, total: i16) -> ClassLimits;
    /// The area count a layout gives a default list.
    fn layout_areas(&mut self, layout: u32) -> u8;
    fn target_reference(&mut self, query: &ReferenceQuery) -> u32;
    fn target_level(&mut self, query: &LevelQuery) -> i16;
    fn subject_eligible(&mut self, query: SubjectQuery) -> bool;
    fn focus_blend(&mut self, query: &BlendQuery) -> u32;
    /// Splits a level at its ceiling into the kept level and the excess.
    fn settle_level(&mut self, level: i16) -> (i16, i16);
    /// Raises a level to its floor.
    fn floor_level(&mut self, level: i16) -> i16;
}

/// The image layers' class and cell codes.
pub trait GridImaging {
    fn class_code(&mut self, class: SelectionClass) -> u8;
    fn code_class(&mut self, code: u8) -> SelectionClass;
    /// The cell a chosen class selects.
    fn class_cell(&mut self, class: SelectionClass) -> CellCode;
    /// The column code a fixed class takes.
    fn cell_code(&mut self, class: SelectionClass) -> u8;
    fn cell_class(&mut self, cell: CellCode) -> u32;
    fn cell_kind(&mut self, cell: CellCode) -> u32;
    fn cell_level(&mut self, cell: CellCode) -> i16;
    fn cell_unset(&mut self, cell: CellCode) -> bool;
    /// The status byte of an entry of the class table.
    fn class_status(&mut self, index: u32) -> u8;
    fn level_scale(&mut self, level: i16) -> u32;
    fn level_class(&mut self, level: i32, value: i32) -> SelectionClass;
    fn exposure_value(&mut self) -> i32;
    fn first_class_index(&mut self) -> u32;
    fn class_level(&mut self, index: u32) -> u16;
    fn class_changed(&mut self, query: &ClassQuery) -> bool;
    fn record_class(&mut self, query: &ClassQuery) -> u32;
    fn record_mode(&mut self, record: RecordHandle) -> u32;
    fn record_flags(&mut self, record: RecordHandle) -> u16;
    fn default_class(&mut self, query: &DefaultQuery) -> SelectionClass;
}

pub trait GridSettings {
    fn setting(&mut self, setting: GridSetting) -> i32;
    fn lens_threshold(&mut self, step: u8) -> i16;
    fn lens_band(&mut self, level: i16) -> u32;
    /// The level of an area's reference at the lens reference.
    fn reference_level(&mut self, reference: u32, lens_reference: u32) -> i16;
    /// The position offset of `reference` from the first reference.
    fn reference_offset(&mut self, reference: u32) -> i16;
}

pub trait GridLayers: GridProfiles + GridLibrary + GridImaging + GridSettings {}
impl<T: GridProfiles + GridLibrary + GridImaging + GridSettings> GridLayers for T {}
