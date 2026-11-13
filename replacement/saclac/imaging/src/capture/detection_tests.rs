use std::vec::Vec;

use super::detection::*;
use super::fake::FakeCapture;
use super::ports::{Adjustments, Reference};
use super::{AdjustmentKey, ProfileKind};

struct Fake {
    adjustments: FakeCapture,
    detection: Detection,
    crops: Vec<CropRequest>,
    centered: Rect,
}

impl Fake {
    fn new() -> Self {
        let detection = Detection {
            status: 1,
            count: 2,
            corner: Point { x: 64, y: 27 + 213 },
            size: Point { x: 100, y: 60 },
            center: Point { x: 128, y: 27 + 426 },
            ..Detection::default()
        };
        let centered = Rect { origin: Point { x: 30, y: 20 }, extent: Extent { height: 852, width: 1280 } };
        Self { adjustments: FakeCapture::new(ProfileKind::Tabled, Reference::default()), detection, crops: Vec::new(), centered }
    }
}

impl Adjustments for Fake {
    fn byte(&self, key: AdjustmentKey) -> u8 { self.adjustments.byte(key) }
}

impl DetectionPorts for Fake {
    fn detection(&mut self) -> Detection { self.detection }

    fn crop(&mut self, request: &CropRequest) -> Crop {
        self.crops.push(*request);
        let origin = if request.ratio == [3, 4] { Point { x: 10, y: 5 } } else { Point { x: 0, y: 0 } };
        Crop { origin, frame: [1, 2], window: Rect { origin: Point { x: 6, y: 4 }, extent: Extent { height: 426, width: 640 } } }
    }

    fn centered(&mut self, _: &Crop, _: &Detection) -> Rect { self.centered }
}

#[test]
fn a_detection_is_placed_in_the_crop_at_its_centred_offset() {
    let mut fake = Fake::new();
    let (placement, framing) = place_detection(&mut fake, 4, [7, 8, 9]).unwrap();
    // Doubled from 640 x 426, then moved by (30 - 10, 20 - 5).
    let placed = PlacedBox { count: 2, corner: Point { x: 148, y: 441 }, extent: Extent { height: 852, width: 256 }, center: Point { x: 276, y: 867 } };
    assert_eq!(placement, Placement { status: 1, state: 0, placed: Some(placed) });
    assert_eq!(framing, Some(Framing { frame: [1, 2], offset: Point { x: 20, y: 15 }, extent: Extent { height: 852, width: 1280 } }));
    assert_eq!(fake.crops, [CropRequest { profile: 4, area: [7, 8, 9], ratio: [3, 4], inset: true }]);
}

#[test]
fn an_unusable_result_reports_no_box_and_a_wide_profile_is_refused() {
    let mut fake = Fake::new();
    fake.detection.state = 1;
    assert_eq!(place_detection(&mut fake, 4, [0; 3]), Ok((Placement { status: 1, state: 1, placed: None }, None)));
    assert_eq!(place_detection(&mut fake, 36, [0; 3]), Err(Refused));
    assert!(fake.crops.is_empty());
}

#[test]
fn a_grown_box_is_kept_to_the_visible_frame() {
    let mut fake = Fake::new();
    fake.adjustments.set_byte(0xa4e2, 100);
    fake.adjustments.set_byte(0xa4e0, 200);
    fake.adjustments.set_byte(0xa4e1, 50);
    let (placement, _) = place_grown_detection(&mut fake, 4, [0; 3], Visible::SixteenByNine).unwrap();
    // Across: 128 -/+ 50, kept to 0..=640; down: 453 - 60 below, kept to 60..=360; then doubled.
    let placed = placement.placed.unwrap();
    assert_eq!(placed.corner, Point { x: 2 * 78 + 20, y: 2 * (393 - 27) + 15 });
    assert_eq!(placed.extent, Extent { height: 4 * (426 - 366), width: 4 * (128 - 78) });
}

#[test]
fn a_rescaled_detection_moves_by_the_second_crop_window() {
    let mut fake = Fake::new();
    let placement = rescale_detection(&mut fake, [7, 8, 9], [16, 9], Extent { height: 213, width: 320 });
    // In the first crop's 426 x 640 window unchanged, moved by (30 - 6, 20 - 4), then halved.
    let placed = placement.placed.unwrap();
    assert_eq!(placed.corner, Point { x: (64 + 24) / 2, y: (213 + 16) / 2 });
    assert_eq!(placed.center, Point { x: (128 + 24) / 2, y: (426 + 16) / 2 });
    assert_eq!(fake.crops.iter().map(|request| (request.profile, request.ratio, request.inset)).collect::<Vec<_>>(), [(1, [3, 4], false), (1, [16, 9], false)]);
}
