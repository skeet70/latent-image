//! X-M1 1.50 values of the capture's calc requests: the configuration ids they read through
//! 0x69f8fc and the layout of the capture pipeline's processing configuration.

use xm1_saclac_interface::calc::CaptureProcessing;
use xm1_saclac_profile_records::Index;

use crate::exposure::ports::Property;

/// Added to the record's chosen word while the scene flags are 2 (0xf51bb8).
pub const CAPTURE_OFFSET_ADJUSTMENT: Property = Property(0x48a3);
/// Nonzero keeps record field 0x1ec in a second calculation whose offset mode is 2 (0xfde654).
pub const KEEPS_OFFSET_FIELD: Property = Property(0x8300);

/// The scene a refinement record measures a person variant of a scene as: nonzero keeps the
/// scene without the person, zero takes portrait (0xfde198..0xfde23c).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefinedScene {
    NightPortrait,
    Sunset,
    Snow,
    Beach,
    Landscape,
}

pub const fn refined_scene(scene: RefinedScene) -> Property {
    Property(match scene {
        RefinedScene::NightPortrait => 0x7951,
        RefinedScene::Sunset => 0x7952,
        RefinedScene::Snow => 0x7953,
        RefinedScene::Beach => 0x7954,
        RefinedScene::Landscape => 0x7955,
    })
}

/// The element of the record's three-element arrays the capture's requests read.
pub const FIRST_ELEMENT: Index<3> = match Index::new(0) {
    Some(index) => index,
    None => panic!("0 indexes a three-element array"),
};

pub const PROCESSING_BYTES: usize = 0x98;

/// The processing configuration as 0xf4ca60, 0xf4d350, 0xf4d448, 0xf51bcc and 0xf52120 read it
/// through their argument's +4 word.
pub fn processing(bytes: &[u8; PROCESSING_BYTES]) -> CaptureProcessing {
    let word = |offset: usize| u32::from_le_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
    let half = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
    let mut geometry = [0; 24];
    geometry.copy_from_slice(&bytes[0x10..0x28]);
    CaptureProcessing {
        extent: [word(0x00), word(0x04)],
        grid: [word(0x08), word(0x0c)],
        geometry,
        level: half(0x64),
        reference: half(0x66),
        decade: half(0x6a),
        companion: half(0x6c),
        setting: word(0x70),
        profile: word(0x74),
        lens: word(0x78),
        lens_reference: word(0x7c),
        offset_mode: word(0x88),
    }
}
