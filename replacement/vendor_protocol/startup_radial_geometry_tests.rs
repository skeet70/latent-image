extern crate std;

use super::{build_primary, build_secondary, RadialRecord};
use super::super::backend::{BackendError, ConfigImages};
use std::vec;

#[test]
fn neutral_primary_publishes_identity_knots_and_final_channel_metadata() {
    let mut encoded = vec![0; 0x18f46];
    let config = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut record = RadialRecord::new();
    assert_eq!(build_primary(&config, &[0; 8], 1000, &mut record), Ok(true));
    assert_eq!(record.knots(), &[65536; 31]);
    assert_eq!(record.metadata(), &[2, 2, 992, 992, 4096, 4096, 4, 0]);
    assert!(record.valid());
}

#[test]
fn secondary_does_not_change_the_primary_grid_selection() {
    let mut encoded = vec![0; 0x18f46];
    let config = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut record = RadialRecord::new();
    assert_eq!(build_secondary(&config, &[0; 8], 1000, &mut record), Ok(true));
    assert_eq!(record.metadata(), &[2, 2, 992, 992, 4096, 4096, 0, 0]);
    assert_eq!(build_primary(&config, &[0; 8], 1000, &mut record), Ok(true));
    assert_eq!(build_secondary(&config, &[0; 8], 1000, &mut record), Ok(true));
    assert_eq!(record.metadata()[6], 4);
}

#[test]
fn fractional_strength_expands_the_first_transition_and_constant_tail() {
    let mut encoded = vec![0; 0x18f46];
    let config = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut record = RadialRecord::new();
    assert_eq!(build_secondary(&config, &[16; 8], 1, &mut record), Ok(true));
    assert_eq!(&record.knots()[..5], &[65536, 65536, 65537, 65537, 65537]);
    assert_eq!(&record.knots()[5..], &[65537; 26]);
    assert!(record.valid());
}

#[test]
fn zero_strength_ignores_signed_sample_bits() {
    let mut encoded = vec![0; 0x18f46];
    let config = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut record = RadialRecord::new();
    assert_eq!(build_secondary(&config, &[0x8000; 8], 0, &mut record), Ok(true));
    assert_eq!(record.knots(), &[65536; 31]);
    assert_eq!(record.gain(), 4096);
    assert_eq!(record.reciprocal(), 4096);
}

#[test]
fn missing_mode_configuration_fails_after_expanding_the_curve() {
    let mut encoded = [];
    let config = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut record = RadialRecord::new();
    assert_eq!(build_secondary(&config, &[0; 8], 1000, &mut record), Err(BackendError::Capacity));
    assert_eq!(record.knots(), &[65536; 31]);
    assert!(!record.valid());
}

#[test]
fn clearing_removes_curve_metadata_and_validity() {
    let mut encoded = vec![0; 0x18f46];
    let config = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut record = RadialRecord::new();
    assert_eq!(build_primary(&config, &[0; 8], 1000, &mut record), Ok(true));
    record.clear();
    assert_eq!(record.knots(), &[0; 31]);
    assert_eq!(record.metadata(), &[0; 8]);
    assert!(!record.valid());
}
