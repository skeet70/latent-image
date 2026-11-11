#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Point {
    pub vertical: u32,
    pub horizontal: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Rectangle {
    pub origin: Point,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipPolicy {
    Intersection,
    MinimumArea { percentage: u32 },
    InteriorCentre,
}

impl Rectangle {
    pub fn clip(&mut self, minimum: Point, maximum: Point) {
        let right = self.origin.horizontal.wrapping_add(self.width);
        let bottom = self.origin.vertical.wrapping_add(self.height);
        if self.origin.horizontal < minimum.horizontal {
            self.width = self.width.wrapping_sub(minimum.horizontal.wrapping_sub(self.origin.horizontal));
            self.origin.horizontal = minimum.horizontal;
        }
        if maximum.horizontal < right {
            self.width = self.width.wrapping_sub(right.wrapping_sub(maximum.horizontal));
        }
        if self.origin.vertical < minimum.vertical {
            self.height = self.height.wrapping_sub(minimum.vertical.wrapping_sub(self.origin.vertical));
            self.origin.vertical = minimum.vertical;
        }
        if maximum.vertical < bottom {
            self.height = self.height.wrapping_sub(bottom.wrapping_sub(maximum.vertical));
        }
    }

    pub fn clipped(self, centre: Point, minimum: Point, maximum: Point, dimensions: Point, policy: ClipPolicy) -> Option<Self> {
        if self.origin.horizontal.wrapping_add(self.width) < minimum.horizontal
            || maximum.horizontal < self.origin.horizontal
            || self.origin.vertical.wrapping_add(self.height) < minimum.vertical
            || maximum.vertical < self.origin.vertical {
            return None;
        }
        if policy == ClipPolicy::InteriorCentre && (centre.vertical <= minimum.vertical
            || maximum.vertical <= centre.vertical || centre.horizontal <= minimum.horizontal
            || maximum.horizontal <= centre.horizontal) {
            return None;
        }
        let mut result = self;
        result.clip(minimum, maximum);
        if let ClipPolicy::MinimumArea { percentage } = policy {
            let area = dimensions.vertical.wrapping_mul(dimensions.horizontal);
            if result.width.wrapping_mul(result.height) < percentage.wrapping_mul(area) / 100 {
                return None;
            }
        }
        Some(result)
    }
}

pub fn grid_index(position: u32, origin: u32, stride: u32, cells: i16) -> i16 {
    let count = cells.wrapping_add(1) as u16;
    if count == 0 || position < origin { return -1; }
    let mut index = 0u16;
    let mut result = -1i16;
    loop {
        if u32::from(index) == u32::from(count) - 1 && result == -1 { result = index as i16; }
        index = index.wrapping_add(1);
        if i32::from(index as i16) >= i32::from(count) { return result; }
        if position < origin.wrapping_add(stride.wrapping_mul(index as i16 as u32)) {
            return index.wrapping_sub(1) as i16;
        }
    }
}
