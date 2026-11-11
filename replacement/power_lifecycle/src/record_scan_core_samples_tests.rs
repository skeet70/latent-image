use super::record_scan_core_samples::{RankLimit, SampleCell, SamplePair, reduce_samples, select_cell};

#[test]
fn cell_coordinates_are_halved_and_column_major() {
    let mut samples = [0; 128];
    samples[19] = 37;
    samples[83] = 91;
    let cell = SampleCell::from_doubled(7, 5).unwrap();
    assert_eq!(select_cell(&samples, cell), SamplePair { primary: 91, secondary: 37 });
    assert!(SampleCell::from_doubled(14, 0).is_none());
    assert!(SampleCell::from_doubled(0, 14).is_none());
    assert!(SampleCell::from_doubled(u32::MAX, 0).is_none());
}

#[test]
fn averages_only_cells_at_or_below_the_requested_rank() {
    let mut samples = [u32::MAX; 128];
    let mut ranks = [4; 64];
    ranks[0] = 2;
    ranks[1] = 3;
    samples[0] = 10;
    samples[1] = 15;
    samples[64] = 40;
    samples[65] = 47;
    assert_eq!(reduce_samples(&samples, &ranks, RankLimit(3), None), SamplePair { primary: 43, secondary: 12 });
}

#[test]
fn weights_control_the_divisor_and_each_product_wraps_to_a_word() {
    let mut samples = [0; 128];
    let mut weights = [0; 64];
    samples[0] = 11;
    samples[1] = 20;
    samples[64] = 0xffff_ffff;
    samples[65] = 5;
    weights[0] = 2;
    weights[1] = 1;
    assert_eq!(reduce_samples(&samples, &[0; 64], RankLimit(0), Some(&weights)), SamplePair { primary: 1_431_655_766, secondary: 14 });
}

#[test]
fn empty_rank_and_zero_weight_sets_produce_zero() {
    let samples = [u32::MAX; 128];
    assert_eq!(reduce_samples(&samples, &[1; 64], RankLimit(0), None), SamplePair { primary: 0, secondary: 0 });
    assert_eq!(reduce_samples(&samples, &[0; 64], RankLimit(0), Some(&[0; 64])), SamplePair { primary: 0, secondary: 0 });
}

#[test]
fn accumulation_keeps_carries_above_the_low_word() {
    let pair = reduce_samples(&[u32::MAX; 128], &[0; 64], RankLimit(0), None);
    assert_eq!(pair, SamplePair { primary: u32::MAX, secondary: u32::MAX });
}
