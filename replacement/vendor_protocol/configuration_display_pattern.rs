//! Pattern fill recovered from 0x00dbb9c4 at x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternGeometry { Standard, WidePanel, FullHd }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PatternLayout { pub width: u16, pub height: u16, pub stride: u16 }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PatternError { Capacity }

impl PatternGeometry {
    pub fn from_device(category: u8, timing: u8) -> Self {
        match category {
            0xcb => Self::WidePanel,
            0xcf if matches!(timing, 0xd5..=0xd9) => Self::FullHd,
            _ => Self::Standard,
        }
    }

    pub const fn layout(self) -> PatternLayout {
        match self {
            Self::Standard => PatternLayout { width: 640, height: 480, stride: 640 },
            Self::WidePanel => PatternLayout { width: 800, height: 600, stride: 800 },
            Self::FullHd => PatternLayout { width: 1920, height: 1080, stride: 1920 },
        }
    }

    pub fn fill(self, destination: &mut [u32]) -> Result<PatternLayout, PatternError> {
        let layout = self.layout();
        let words_per_row = layout.stride as usize / 2;
        let required = words_per_row * layout.height as usize;
        let destination = destination.get_mut(..required).ok_or(PatternError::Capacity)?;
        if self == Self::FullHd {
            let bars = [0x8080_ffff, 0x8b00_ecec, 0x009d_c8c8, 0x0c1d_b6b6, 0xf3e2_4848, 0xff62_3636, 0x74ff_1212, 0x8080_0000];
            for row in destination.chunks_exact_mut(words_per_row) {
                for (index, word) in row.iter_mut().enumerate() { *word = bars[index / 120]; }
            }
        } else {
            let width = layout.width as usize;
            let upper_rows = width * 3 / 8;
            let upper_bar = width / 16;
            let lower_bar = width / 10;
            let upper = [0x8080_a3a3, 0x8d2b_9191, 0x2b9b_7272, 0x3948_5f5f, 0xc6b6_4343, 0xd363_3030, 0x71d3_1212, 0x8080_0000];
            let lower = [0x8080_0000, 0x8080_0a0a, 0x8080_8080, 0x8080_ffff, 0x8080_dcdc];
            for (y, row) in destination.chunks_exact_mut(words_per_row).enumerate() {
                for (x, word) in row.iter_mut().enumerate() {
                    *word = if y < upper_rows {
                        if x < upper_bar * 8 { upper[x / upper_bar] } else { 0x8080_ffff }
                    } else if x < lower_bar * 5 { lower[x / lower_bar] } else { 0x8080_ffff };
                }
            }
        }
        Ok(layout)
    }
}

pub fn solid_pattern(chroma: u8, first_luma: u8, second_luma: u8) -> u32 {
    (u32::from(chroma) << 24) | (u32::from(chroma) << 16) | (u32::from(second_luma) << 8) | u32::from(first_luma)
}

#[cfg(test)]
mod tests {
    use super::{PatternError, PatternGeometry, solid_pattern};

    #[test]
    fn pattern_rejects_short_destination_without_partial_writes() {
        let mut bytes = [0x1234_5678; 4];
        assert_eq!(PatternGeometry::Standard.fill(&mut bytes), Err(PatternError::Capacity));
        assert_eq!(bytes, [0x1234_5678; 4]);
    }

    #[test]
    fn solid_pattern_keeps_the_two_lumas_and_repeated_chroma() {
        assert_eq!(solid_pattern(0x80, 0x12, 0x34), 0x8080_3412);
    }

    #[test]
    fn standard_frame_places_bars_and_lower_greys_at_observed_boundaries() {
        let mut frame = [0; 640 * 480 / 2];
        let layout = PatternGeometry::Standard.fill(&mut frame).unwrap();
        assert_eq!((layout.width, layout.height, layout.stride), (640, 480, 640));
        assert_eq!(frame[0], 0x8080_a3a3);
        assert_eq!(frame[39], 0x8080_a3a3);
        assert_eq!(frame[40], 0x8d2b_9191);
        assert_eq!(frame[319], 0x8080_0000);
        assert_eq!(frame[239 * 320], 0x8080_a3a3);
        assert_eq!(frame[240 * 320], 0x8080_0000);
        assert_eq!(frame[240 * 320 + 64], 0x8080_0a0a);
        assert_eq!(frame[479 * 320 + 319], 0x8080_dcdc);
    }

    #[test]
    fn category_and_timing_select_observed_geometry() {
        assert_eq!(PatternGeometry::from_device(0xcf, 0xd4).layout().width, 640);
        assert_eq!(PatternGeometry::from_device(0xcf, 0xd5).layout().width, 1920);
        assert_eq!(PatternGeometry::from_device(0xcb, 0).layout().width, 800);
    }
}
