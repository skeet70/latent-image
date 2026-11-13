//! Rectangles on the image frame: scaling between frames, clipping to bounds and grid cells.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Point {
    pub left: u32,
    pub top: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Rectangle {
    pub left: u32,
    pub top: u32,
    pub height: u32,
    pub width: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame {
    pub vertical: u32,
    pub horizontal: u32,
}

/// A rectangle with the centre its owner keeps beside it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Placement {
    pub rectangle: Rectangle,
    pub centre: Point,
}

/// A placement with the scale that follows its width when it is clipped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Region {
    pub placement: Placement,
    pub scale: u16,
}

impl Rectangle {
    pub const fn centre(self) -> Point {
        Point { left: self.left.wrapping_add(self.width / 2), top: self.top.wrapping_add(self.height / 2) }
    }

    /// The longer side as a percentage of the frame's matching side; `None` when that side of
    /// the frame is zero.
    pub fn percent_of(self, frame: Frame) -> Option<u32> {
        if self.height <= self.width {
            self.width.wrapping_mul(100).checked_div(frame.horizontal)
        } else {
            self.height.wrapping_mul(100).checked_div(frame.vertical)
        }
    }

    /// Cuts the rectangle to `minimum..=maximum` on each axis.
    pub fn clip(&mut self, minimum: Point, maximum: Point) {
        let bottom = self.top.wrapping_add(self.height);
        let right = self.left.wrapping_add(self.width);
        if self.top < minimum.top {
            self.height = self.height.wrapping_sub(minimum.top - self.top);
            self.top = minimum.top;
        }
        if bottom > maximum.top {
            self.height = self.height.wrapping_sub(bottom - maximum.top);
        }
        if self.left < minimum.left {
            self.width = self.width.wrapping_sub(minimum.left - self.left);
            self.left = minimum.left;
        }
        if right > maximum.left {
            self.width = self.width.wrapping_sub(right - maximum.left);
        }
    }
}

/// Moves a placement from frame `from` to frame `to`; no placement gives the zero placement.
/// `None` when a side of `from` is zero.
pub fn rescale(placement: Option<Placement>, from: Frame, to: Frame) -> Option<Placement> {
    let Some(placement) = placement else { return Some(Placement::default()) };
    let horizontal = to.horizontal.wrapping_mul(100).checked_div(from.horizontal)?;
    let vertical = to.vertical.wrapping_mul(100).checked_div(from.vertical)?;
    let source = placement.rectangle;
    let rectangle = Rectangle {
        left: source.left.wrapping_mul(horizontal) / 100,
        top: source.top.wrapping_mul(vertical) / 100,
        height: source.height.wrapping_mul(vertical) / 100,
        width: source.width.wrapping_mul(horizontal) / 100,
    };
    Some(Placement { rectangle, centre: rectangle.centre() })
}

/// How much of a region must lie inside the bounds for it to be kept.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipRule {
    /// Any overlap.
    Overlap,
    /// The visible part covers at least the configured share of the frame.
    VisibleShare,
    /// The centre lies strictly inside the bounds.
    CentreInside,
}

pub trait ClipSettings {
    fn clip_rule(&mut self) -> ClipRule;
    /// The share of the frame, in percent, `ClipRule::VisibleShare` requires.
    fn minimum_visible_percent(&mut self) -> u32;
}

/// Clips a region to `minimum..=maximum` and recentres it; false leaves it unchanged.
/// A region of zero width is refused where stock traps on the division (0x43614).
pub fn clip_region(region: &mut Region, minimum: Point, maximum: Point, frame: Frame, settings: &mut impl ClipSettings) -> bool {
    let rule = settings.clip_rule();
    let Placement { rectangle, centre } = region.placement;
    if rectangle.top.wrapping_add(rectangle.height) < minimum.top
        || rectangle.top > maximum.top
        || rectangle.left.wrapping_add(rectangle.width) < minimum.left
        || rectangle.left > maximum.left
    {
        return false;
    }
    if rule == ClipRule::CentreInside
        && (minimum.left >= centre.left || maximum.left <= centre.left || minimum.top >= centre.top || maximum.top <= centre.top)
    {
        return false;
    }
    let mut clipped = rectangle;
    clipped.clip(minimum, maximum);
    if rule == ClipRule::VisibleShare {
        let frame_area = frame.vertical.wrapping_mul(frame.horizontal);
        let required = settings.minimum_visible_percent().wrapping_mul(frame_area) / 100;
        if clipped.height.wrapping_mul(clipped.width) < required {
            return false;
        }
    }
    let Some(scale) = clipped.width.wrapping_mul(u32::from(region.scale)).checked_div(rectangle.width) else { return false };
    region.scale = scale as u16;
    region.placement = Placement { rectangle: clipped, centre: clipped.centre() };
    true
}

/// A row of equal cells from `origin`, with a gap after each.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grid {
    pub origin: Point,
    pub cells: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub gap: Frame,
}

/// The cell under a point on each axis; `None` before the origin or for an empty grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub column: Option<u16>,
    pub row: Option<u16>,
}

impl Grid {
    /// A point past the last cell is in the last cell. Stock's scan (0xf4ce44) does not end for
    /// more than 32767 cells.
    pub fn cell_at(&self, point: Point) -> Cell {
        let axis = |position: u32, origin: u32, step: u32| -> Option<u16> {
            if self.cells == 0 || position < origin {
                return None;
            }
            let past = (1..self.cells).find(|&cell| position < step.wrapping_mul(u32::from(cell)).wrapping_add(origin));
            Some(past.map_or(self.cells - 1, |cell| cell - 1))
        };
        Cell {
            column: axis(point.left, self.origin.left, u32::from(self.cell_width).wrapping_add(self.gap.horizontal)),
            row: axis(point.top, self.origin.top, u32::from(self.cell_height).wrapping_add(self.gap.vertical)),
        }
    }
}
