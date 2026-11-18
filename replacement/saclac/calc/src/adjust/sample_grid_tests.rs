use crate::adjust::sample_grid::*;

fn samples() -> [u32; GRID_SAMPLES] { core::array::from_fn(|index| if index < 64 { index as u32 } else { 1000 + (index - 64) as u32 }) }

#[test]
fn a_cell_takes_its_pair_at_half_its_coordinates() {
    assert_eq!(grid_pair(&samples(), GridSelection::Cell([5, 4])), Ok((1000 + 2 + 16, 2 + 16)));
    assert_eq!(grid_pair(&samples(), GridSelection::Cell([14, 0])), Err(Refused));
    assert_eq!(grid_pair(&samples(), GridSelection::Cell([0, 14])), Err(Refused));
}

#[test]
fn the_centre_ring_is_the_centre_cell() {
    assert_eq!(grid_pair(&samples(), GridSelection::Rings { limit: 0, weights: None }), Ok((1027, 27)));
}

#[test]
fn rings_average_the_cells_within_them() {
    let inner = [18u32, 19, 20, 26, 27, 28, 34, 35, 36];
    let sum: u32 = inner.iter().sum();
    assert_eq!(grid_pair(&samples(), GridSelection::Rings { limit: 1, weights: None }), Ok(((sum + 9000) / 9, sum / 9)));
    let all = grid_pair(&samples(), GridSelection::Rings { limit: 3, weights: None }).unwrap();
    let cells: std::vec::Vec<u32> = (0..64).filter(|cell| cell % 8 != 7 && cell / 8 != 7).collect();
    assert_eq!(all.1, cells.iter().sum::<u32>() / cells.len() as u32);
}

#[test]
fn weights_count_cells_that_many_times_and_no_weight_divides_by_one() {
    let mut weights = [0u8; 64];
    weights[27] = 3;
    weights[18] = 1;
    assert_eq!(grid_pair(&samples(), GridSelection::Rings { limit: 1, weights: Some(&weights) }), Ok(((3 * 1027 + 1018) / 4, (3 * 27 + 18) / 4)));
    assert_eq!(grid_pair(&samples(), GridSelection::Rings { limit: 1, weights: Some(&[0; 64]) }), Ok((0, 0)));
}

#[test]
fn only_scenes_47_and_56_carry_an_offset() {
    assert!(scene_offset_valid(0, 47, 5));
    assert!(scene_offset_valid(2, 56, -5));
    assert!(!scene_offset_valid(1, 47, 5));
    assert!(!scene_offset_valid(0, 47, i16::MIN));
    assert!(!scene_offset_valid(0, 46, 5));
}
