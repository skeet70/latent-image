//! Settings the shot and the exposure engine read by the record's scene kind: the nibble pair,
//! whether the scene's adjustment is enabled, the range around the target level and the curve
//! table calc reads.

use xm1_saclac_profile_records::MeasurementSelection;

use crate::exposure::ports::{Properties, Property};
use crate::exposure::settings;
use crate::stock::scene::{self, RangeSettings, SceneEnable};

/// Selection flags bit 1 replaces every scene setting with a fixed one.
const FLAGGED: u16 = 2;

/// The record's selection as the scene settings read it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneSelection {
    pub measurement_selection: MeasurementSelection,
    /// The selection kind (0xf612dc).
    pub scene: u32,
    /// The selection flags (0xf61584).
    pub flags: u16,
}

impl SceneSelection {
    const fn flagged(&self) -> bool { self.flags & FLAGGED != 0 }

    fn selected(&self) -> bool { self.measurement_selection == MeasurementSelection::Record }
}

/// What chooses the nibble pair beside the scene.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NibbleInputs {
    pub scene: u32,
    pub flags: u16,
    /// The measured level (0xf5fc00).
    pub measured_level: i16,
    /// Record byte 0x45.
    pub policy: u8,
    /// Shared header byte 0x63.
    pub armed: bool,
    /// Record byte 0x51.
    pub selector: u8,
    /// Record byte 0x3e; 0 leaves the choice to the scene.
    pub choice: u8,
}

/// Two four-bit values packed in one configuration byte.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Nibbles {
    pub low: u8,
    pub high: u8,
}

/// The nibble pair of a record (0xf6ec88).
pub fn nibbles(properties: &mut impl Properties, inputs: NibbleInputs) -> Nibbles {
    let setting = if inputs.flags & FLAGGED != 0 {
        scene::FLAGGED_NIBBLES
    } else if inputs.selector == 4 {
        scene::SELECTOR_NIBBLES
    } else if inputs.armed && matches!(inputs.scene, 0 | 48) {
        scene::ARMED_NIBBLES
    } else if inputs.choice != 0 {
        scene::override_nibbles(inputs.choice)
    } else {
        scene::scene_nibbles(inputs.scene, inputs.measured_level != 0, inputs.policy)
    };
    let byte = properties.property(setting) as u8;
    Nibbles { low: byte & 0xf, high: byte >> 4 }
}

/// What decides whether a scene's adjustment is enabled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EnableInputs {
    pub selection: SceneSelection,
    /// The selection auxiliary byte (0xf6171c); any value disables the adjustment.
    pub auxiliary: u8,
    /// Record word 0x14; any value disables scenes 44 and 57.
    pub hold: u32,
    /// Record byte 0x51.
    pub selector: u8,
}

/// Whether the record's scene has its adjustment enabled (0xf6efcc).
pub fn adjustment_enabled(properties: &mut impl Properties, inputs: EnableInputs) -> bool {
    let selection = inputs.selection;
    if inputs.auxiliary != 0 || selection.selected() {
        return false;
    }
    let bit = if selection.flagged() {
        Some(scene::FLAGGED_ENABLE)
    } else {
        match scene::scene_enable(selection.scene) {
            SceneEnable::Always => return true,
            SceneEnable::Never => None,
            SceneEnable::Bit(bit) => Some(bit),
            SceneEnable::BySelector if inputs.hold != 0 => None,
            SceneEnable::BySelector => scene::selector_enable(inputs.selector),
        }
    };
    bit.is_some_and(|bit| properties.property(bit.setting) as u8 & bit.mask != 0)
}

/// The configuration table calc reads for the record's scene (0xf6fa5c).
pub fn curve_table(selection: SceneSelection) -> Property {
    if selection.flagged() || selection.selected() { scene::DEFAULT_CURVE_TABLE } else { scene::curve_table(selection.scene) }
}

/// What chooses the range around the target level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RangeInputs {
    pub selection: SceneSelection,
    /// Record byte 0x34, read for a record selected for measurement.
    pub class: u8,
    /// The measured level was 0.
    pub unmeasured: bool,
    /// Record byte 0x45.
    pub policy: u8,
    /// Shared header byte 0x63.
    pub armed: bool,
    /// Record byte 0x66.
    pub variant: u8,
}

/// The range the exposure engine allows around the target level, in tenths, and its offset.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneRange {
    pub above: u16,
    pub below: u16,
    pub offset: i16,
}

/// The range of the record's scene (0xf6f2d0).
pub fn range(properties: &mut impl Properties, inputs: RangeInputs) -> SceneRange {
    let selection = inputs.selection;
    let settings: RangeSettings = if selection.flagged() {
        scene::FLAGGED_RANGE
    } else if selection.selected() {
        scene::selected_range(u32::from(inputs.class))
    } else if inputs.armed {
        scene::ARMED_RANGE
    } else {
        scene::scene_range(selection.scene, inputs.unmeasured, u32::from(inputs.policy), u32::from(inputs.variant))
    };
    SceneRange {
        above: settings::tenths(properties, settings.above),
        below: settings::tenths(properties, settings.below),
        offset: settings::signed_word(properties, settings.offset),
    }
}
