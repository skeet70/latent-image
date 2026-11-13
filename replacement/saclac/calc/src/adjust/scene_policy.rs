//! Which subject treatments a scene admits, and whether the scene is enabled in the scene mask
//! settings.

use crate::exposure::ports::Properties;
use crate::stock::property;

/// Scene kinds the policy tables cover; a larger kind admits nothing.
pub const SCENES: u32 = 58;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Treatment { First, Second }

/// What decides a scene's treatments.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TreatmentInputs {
    pub active: u32,
    /// Neither treatment applies without subjects.
    pub subjects: u32,
    /// A record selected for measurement uses the fixed selected row in place of its scene's.
    pub measurement_selection: u32,
    pub scene: u32,
    pub class: u32,
    /// An armed record uses the fixed armed row in place of its scene's.
    pub callback_armed: u32,
    pub held: u32,
    pub single_subject: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Column { Other, Unclassed, Ranged, Held, Single }

/// The treatments a row admits per column, as (first, second).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Row { Standard, Wide, SecondOnly, Armed, Closed }

impl Row {
    const fn of_scene(scene: u32) -> Self {
        match scene {
            0 | 43..=47 | 54..=56 => Self::Wide,
            11 | 12 | 25 | 51 | 52 => Self::SecondOnly,
            5..=7 | 10 | 13 | 24 | 28 | 29 | 32 | 34 | 37 | 40 | 53 => Self::Closed,
            49 | 50 => Self::Armed,
            _ if scene < SCENES => Self::Standard,
            _ => Self::Closed,
        }
    }

    const fn admits(self, column: Column) -> (bool, bool) {
        match (self, column) {
            (Self::Closed, _) => (false, false),
            (Self::SecondOnly, Column::Other | Column::Ranged | Column::Single) => (false, true),
            (Self::SecondOnly, _) => (false, false),
            (_, Column::Other | Column::Ranged) => (true, true),
            (Self::Armed, Column::Unclassed) => (true, false),
            (_, Column::Unclassed) => (false, false),
            (Self::Wide, Column::Held) => (false, true),
            (_, Column::Held) => (false, false),
            (_, Column::Single) => (false, true),
        }
    }
}

impl TreatmentInputs {
    fn column(&self) -> Column {
        if self.held != 0 {
            Column::Held
        } else if self.single_subject == 1 {
            Column::Single
        } else if self.class == 0 {
            Column::Unclassed
        } else if (4..=6).contains(&self.class) {
            Column::Ranged
        } else {
            Column::Other
        }
    }

    fn row(&self) -> Row {
        if self.measurement_selection != 0 {
            Row::Standard
        } else if self.callback_armed != 0 {
            Row::Armed
        } else {
            Row::of_scene(self.scene)
        }
    }

    pub fn admits(&self, treatment: Treatment) -> bool {
        if (self.held == 0 && self.active == 0) || self.subjects == 0 {
            return false;
        }
        let (first, second) = self.row().admits(self.column());
        match treatment {
            Treatment::First => first,
            Treatment::Second => second,
        }
    }

    pub fn admits_any(&self) -> bool { self.admits(Treatment::First) || self.admits(Treatment::Second) }
}

/// One bit of the scene mask settings, numbered from the first mask byte's lowest bit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SceneFlag(u8);

impl SceneFlag {
    const SELECTED: Self = Self(14);
    const ARMED: Self = Self(15);
    const OTHER: Self = Self(39);

    const fn of_scene(scene: u32) -> Self {
        const FLAGS: [u8; SCENES as usize] = [
            0, 16, 16, 16, 16, 17, 17, 17, 16, 16, 18, 19, 20, 20, 16, 16, 21, 39, 25, 16, 16, 24, 16, 16, 22, 23, 16, 16,
            26, 29, 28, 27, 30, 39, 17, 16, 16, 17, 16, 16, 17, 16, 16, 3, 3, 8, 10, 12, 39, 32, 33, 34, 35, 36, 10, 8, 12, 3,
        ];
        if scene < SCENES { Self(FLAGS[scene as usize]) } else { Self::OTHER }
    }
}

/// What decides whether a record's scene is enabled.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SceneMaskInputs {
    pub measurement_selection: u32,
    pub scene: u32,
    pub callback_armed: u32,
    /// A treated exposure also needs both treatment settings on and a subject.
    pub treated: bool,
    pub subjects: u32,
}

pub fn scene_enabled(properties: &mut impl Properties, inputs: SceneMaskInputs) -> bool {
    if inputs.treated
        && (properties.property(property::DUMP_SECOND) == 0 || properties.property(property::DUMP_FIRST) == 0 || inputs.subjects == 0)
    {
        return false;
    }
    let SceneFlag(bit) = if inputs.measurement_selection == 1 {
        SceneFlag::SELECTED
    } else if inputs.callback_armed == 1 {
        SceneFlag::ARMED
    } else {
        SceneFlag::of_scene(inputs.scene)
    };
    properties.property(property::scene_mask(bit / 8)) & (1 << (bit % 8)) != 0
}
