#![forbid(unsafe_code)]

//! The subject detector's latest box placed in a capture's frames (0x10510e8, 0x10512cc,
//! 0x10515f4 with 0x1050ec0 and 0x1045240), and the result itself (0x10517a0).

use super::ports::Adjustments;
use super::AdjustmentKey;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Point {
    pub x: u32,
    pub y: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Extent {
    pub height: u32,
    pub width: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub extent: Extent,
}

/// The detector's latest result (task 65's state, read through 0x10451ac).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Detection {
    pub frame: [u32; 2],
    pub status: u32,
    pub state: u32,
    pub reserved: u32,
    pub count: u32,
    pub corner: Point,
    pub size: Point,
    pub center: Point,
}

impl Detection {
    const fn usable(&self) -> bool { self.status == 1 && self.state != 1 }
}

/// A box in a frame: its corner, its centre and twice the distance between them.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PlacedBox {
    pub count: u32,
    pub corner: Point,
    pub extent: Extent,
    pub center: Point,
}

/// What a placement reports: the detector's status and state, and the box when they allow one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Placement {
    pub status: u32,
    pub state: u32,
    pub placed: Option<PlacedBox>,
}

/// The crop frame a placement used and where the detection sits in it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Framing {
    pub frame: [u32; 2],
    pub offset: Point,
    pub extent: Extent,
}

/// A profile-geometry crop request (0xef6030).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CropRequest {
    pub profile: u32,
    pub area: [u32; 3],
    pub ratio: [u16; 2],
    pub inset: bool,
}

/// What 0xef6030 computes for a request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Crop {
    pub origin: Point,
    pub frame: [u32; 2],
    pub window: Rect,
}

/// The part of the detector's frame a capture shows, by the caller's frame mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Visible {
    /// Mode 3.
    ThreeByTwo,
    /// Mode 16.
    SixteenByNine,
    /// Mode 1.
    Square,
    FourByThree,
}

impl Visible {
    pub const fn from_mode(mode: u16) -> Self {
        match mode {
            3 => Self::ThreeByTwo,
            16 => Self::SixteenByNine,
            1 => Self::Square,
            _ => Self::FourByThree,
        }
    }

    /// The least and greatest coordinate a box keeps.
    const fn bounds(self) -> (Point, Point) {
        match self {
            Self::ThreeByTwo => (Point { x: 0, y: 27 }, Point { x: 640, y: 0x1aa }),
            Self::SixteenByNine => (Point { x: 0, y: 60 }, Point { x: 640, y: 360 }),
            Self::Square => (Point { x: 107, y: 27 }, Point { x: 0x1aa, y: 0x1aa }),
            Self::FourByThree => (Point { x: 0, y: 0 }, Point { x: 640, y: 480 }),
        }
    }
}

/// A placement refused for its arguments; stock answers 0x84.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refused;

pub trait DetectionPorts: Adjustments {
    /// The detector's latest result; all zero when its lock is refused (0x10451ac).
    fn detection(&mut self) -> Detection;
    /// 0xef6030.
    fn crop(&mut self, request: &CropRequest) -> Crop;
    /// The detection's frame centred and scaled into the crop (0xedb0dc).
    fn centered(&mut self, crop: &Crop, detection: &Detection) -> Rect;
}

/// The extent a detected box is measured in, `BORDER` rows below the detector frame's top.
const DETECTOR: Extent = Extent { height: 0x1aa, width: 640 };
const BORDER: u32 = 27;
/// The highest profile handle.
const LAST_PROFILE: u32 = 35;
/// Percentages a box grows by: around its centre across, below and above its centre down.
const GROWTH_ACROSS: AdjustmentKey = AdjustmentKey(0xa4e2);
const GROWTH_BELOW: AdjustmentKey = AdjustmentKey(0xa4e0);
const GROWTH_ABOVE: AdjustmentKey = AdjustmentKey(0xa4e1);

/// `value * to / from`; a zero `from` gives 0.
const fn rescale(value: u32, to: u32, from: u32) -> u32 {
    match value.wrapping_mul(to).checked_div(from) {
        Some(scaled) => scaled,
        None => 0,
    }
}

/// The box from corner to centre, rescaled from one frame's extent to another's (0x1045240).
fn rescale_box(count: u32, corner: Point, center: Point, from: Extent, to: Extent) -> PlacedBox {
    let corner = Point { x: rescale(corner.x, to.width, from.width), y: rescale(corner.y, to.height, from.height) };
    let center = Point { x: rescale(center.x, to.width, from.width), y: rescale(center.y, to.height, from.height) };
    let extent = Extent { height: center.y.wrapping_sub(corner.y).wrapping_mul(2), width: center.x.wrapping_sub(corner.x).wrapping_mul(2) };
    PlacedBox { count, corner, extent, center }
}

fn shifted(point: Point, offset: Point) -> Point { Point { x: point.x.wrapping_add(offset.x), y: point.y.wrapping_add(offset.y) } }

fn moved(placed: PlacedBox, offset: Point) -> PlacedBox {
    PlacedBox { corner: shifted(placed.corner, offset), center: shifted(placed.center, offset), ..placed }
}

/// The detection's box from the detector's frame, its border removed, into `to`.
fn from_detector(detection: &Detection, to: Extent) -> PlacedBox {
    let unbordered = |point: Point| Point { x: point.x, y: point.y.wrapping_sub(BORDER) };
    rescale_box(detection.count, unbordered(detection.corner), unbordered(detection.center), DETECTOR, to)
}

/// Grows the box around its centre by the configured percentages, kept to the visible part.
fn grown(adjustments: &impl Adjustments, detection: &Detection, visible: Visible) -> Detection {
    let growth = |half: u32, key: AdjustmentKey| half.wrapping_mul(u32::from(adjustments.byte(key))) / 100;
    let [half_x, half_y] = [detection.size.x >> 1, detection.size.y >> 1];
    let center = detection.center;
    let low = Point { x: center.x.wrapping_sub(growth(half_x, GROWTH_ACROSS)), y: center.y.wrapping_sub(growth(half_y, GROWTH_BELOW)) };
    let high = Point { x: center.x.wrapping_add(growth(half_x, GROWTH_ACROSS)), y: center.y.wrapping_add(growth(half_y, GROWTH_ABOVE)) };
    let (least, greatest) = visible.bounds();
    let start = |low: u32, least: u32| (low as i32).max(least as i32) as u32;
    let end = |high: u32, greatest: u32| (high as i32).min(greatest as i32) as u32;
    let corner = Point { x: start(low.x, least.x), y: start(low.y, least.y) };
    let size = Point { x: end(high.x, greatest.x).wrapping_sub(corner.x), y: end(high.y, greatest.y).wrapping_sub(corner.y) };
    Detection { corner, size, ..*detection }
}

fn place(ports: &mut impl DetectionPorts, profile: u32, area: [u32; 3], visible: Option<Visible>) -> Result<(Placement, Option<Framing>), Refused> {
    if profile > LAST_PROFILE {
        return Err(Refused);
    }
    let detection = ports.detection();
    if !detection.usable() {
        return Ok((Placement { status: detection.status, state: detection.state, placed: None }, None));
    }
    let crop = ports.crop(&CropRequest { profile, area, ratio: [3, 4], inset: true });
    let centered = ports.centered(&crop, &detection);
    let offset = Point { x: centered.origin.x.wrapping_sub(crop.origin.x), y: centered.origin.y.wrapping_sub(crop.origin.y) };
    let detection = match visible {
        Some(visible) => grown(ports, &detection, visible),
        None => detection,
    };
    let placed = moved(from_detector(&detection, centered.extent), offset);
    let framing = Framing { frame: crop.frame, offset, extent: centered.extent };
    Ok((Placement { status: detection.status, state: detection.state, placed: Some(placed) }, Some(framing)))
}

/// The detection's box in the profile's crop of `area`, with the framing it used (0x10510e8).
pub fn place_detection(ports: &mut impl DetectionPorts, profile: u32, area: [u32; 3]) -> Result<(Placement, Option<Framing>), Refused> {
    place(ports, profile, area, None)
}

/// As `place_detection`, the box first grown and kept to the part of the detector's frame
/// `visible` shows (0x10512cc).
pub fn place_grown_detection(ports: &mut impl DetectionPorts, profile: u32, area: [u32; 3], visible: Visible) -> Result<(Placement, Option<Framing>), Refused> {
    place(ports, profile, area, Some(visible))
}

/// The detection's box in a crop of `area` by `ratio`, rescaled to `to` (0x10515f4). The crops
/// take the detector's status as their profile.
pub fn rescale_detection(ports: &mut impl DetectionPorts, area: [u32; 3], ratio: [u16; 2], to: Extent) -> Placement {
    let detection = ports.detection();
    if !detection.usable() {
        return Placement { status: detection.status, state: detection.state, placed: None };
    }
    let request = CropRequest { profile: detection.status, area, ratio: [3, 4], inset: false };
    let first = ports.crop(&request);
    let second = ports.crop(&CropRequest { ratio, ..request });
    let centered = ports.centered(&first, &detection);
    let in_first = from_detector(&detection, first.window.extent);
    let offset = Point { x: centered.origin.x.wrapping_sub(second.window.origin.x), y: centered.origin.y.wrapping_sub(second.window.origin.y) };
    let in_second = moved(in_first, offset);
    let placed = rescale_box(in_second.count, in_second.corner, in_second.center, second.window.extent, to);
    Placement { status: detection.status, state: detection.state, placed: Some(placed) }
}
