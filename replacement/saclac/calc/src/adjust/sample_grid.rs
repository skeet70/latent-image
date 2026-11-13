//! The round's two values from an 8x8 grid of sample pairs: one cell's pair, or the averages over
//! the cells within a ring distance of the centre.

/// Sixty-four first values, then sixty-four second values, by row of eight.
pub const GRID_SAMPLES: usize = 128;
const CELLS: usize = 64;

/// The ring of a cell around cell (3, 3); the eighth row and column are outside every ring.
fn ring(cell: usize) -> u32 {
    let (column, row) = (cell % 8, cell / 8);
    if column == 7 || row == 7 { u32::MAX } else { column.abs_diff(3).max(row.abs_diff(3)) as u32 }
}

/// How the pair is taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GridSelection<'a> {
    /// The cell at half the given coordinates, column first; refused past cell 6.
    Cell([u32; 2]),
    /// The cells up to `limit` rings out, each counted `weights[cell]` times when weights are given.
    Rings { limit: u32, weights: Option<&'a [u8; CELLS]> },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Refused;

/// The pair as (second half's value, first half's value): stock's first output takes the second
/// half (0xf88dcc).
pub fn grid_pair(samples: &[u32; GRID_SAMPLES], selection: GridSelection) -> Result<(u32, u32), Refused> {
    let (low, high) = samples.split_at(CELLS);
    match selection {
        GridSelection::Cell([column, row]) => {
            let (column, row) = (column >> 1, row >> 1);
            if column > 6 || row > 6 {
                return Err(Refused);
            }
            let cell = (column + 8 * row) as usize;
            Ok((high[cell], low[cell]))
        }
        GridSelection::Rings { limit, weights } => {
            let (mut high_sum, mut low_sum, mut count) = (0u64, 0u64, 0u32);
            for cell in (0..CELLS).filter(|cell| ring(*cell) <= limit) {
                let weight = weights.map_or(1, |weights| u32::from(weights[cell]));
                high_sum += u64::from(high[cell].wrapping_mul(weight));
                low_sum += u64::from(low[cell].wrapping_mul(weight));
                count = count.wrapping_add(weight);
            }
            let count = u64::from(count.max(1));
            Ok(((high_sum / count) as u32, (low_sum / count) as u32))
        }
    }
}

/// Whether scenes 47 and 56 carry an offset; a hold of 1 withholds it (0xf7e364).
pub const fn scene_offset_valid(hold: u32, scene: u32, offset: i16) -> bool { hold != 1 && matches!(scene, 47 | 56) && offset != i16::MIN }
