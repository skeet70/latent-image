use super::{ChannelSetup, initialize_grid_tables, initialize_registered_tables, publish_channel_tables};
use crate::backend::{BackendError, ConfigImages};

#[test]
fn grid_export_covers_three_fifteen_channel_banks_and_publishes_success() {
    let mut bytes = std::vec![0xa5; 0x420000];
    bytes[0x3e3a55..0x3e3d25].fill(0);
    bytes[0x18f40] = 0;
    bytes[0x18f41] = 1;
    bytes[0x18f44] = 0;
    bytes[0x18f46] = 100;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    initialize_grid_tables(&mut configuration).unwrap();
    for row in 0..45 {
        let base = 0x400000 + row * 101;
        for knot in 0..31 {
            assert_eq!([configuration.byte(base + knot * 3), configuration.byte(base + knot * 3 + 1), configuration.byte(base + knot * 3 + 2)], [Some(0), Some(0), Some(1)]);
        }
        for (offset, value) in [0, 16, 0, 16, 0, 0, 0, 0].into_iter().enumerate() { assert_eq!(configuration.byte(base + 93 + offset as u32), Some(value)); }
    }
    assert_eq!(configuration.byte(0x3fffff), Some(0xa5));
    assert_eq!(configuration.byte(0x4011c1), Some(1));
    assert_eq!(configuration.byte(0x4011c2), Some(0xa5));
}

#[test]
fn disabled_registered_tables_do_not_require_search_configuration_or_change_output() {
    let mut bytes = std::vec![0; 0x3e000a];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    assert_eq!(initialize_registered_tables(&mut configuration), Ok(()));
}

#[test]
fn registered_tables_use_six_descriptors_and_leave_other_banks_untouched() {
    let mut bytes = std::vec![0xa5; 0x420000];
    bytes[0x33d0..0x3430].fill(0);
    bytes[0x3e0009] = 1;
    bytes[0x18f40] = 0;
    bytes[0x18f41] = 1;
    bytes[0x18f44] = 0;
    bytes[0x18f46] = 100;
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    initialize_registered_tables(&mut configuration).unwrap();
    for row in 0..6 {
        assert_eq!(configuration.byte(0x403600 + row * 101 + 2), Some(1));
        assert_eq!(configuration.byte(0x403600 + row * 101 + 94), Some(16));
    }
    assert_eq!(configuration.byte(0x4035ff), Some(0xa5));
    assert_eq!(configuration.byte(0x40385e), Some(0xa5));
    assert_eq!(configuration.byte(0x400000), Some(0xa5));
    assert_eq!(configuration.byte(0x4011c1), Some(1));
}

#[test]
fn no_search_attempts_publish_failure_without_fabricating_a_successful_table() {
    let mut bytes = std::vec![0; 0x420000];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    initialize_grid_tables(&mut configuration).unwrap();
    assert_eq!(configuration.byte(0x4011c1), Some(0));
    assert_eq!(configuration.byte(0x40005e), Some(0));
}

#[test]
fn channel_publication_replicates_all_samples_and_low_halfword_metadata() {
    let mut bytes = std::vec![0xa5; 0x420000];
    bytes[0x18f44] = 0;
    let mut setup = ChannelSetup::new();
    for channel in 0..15 { setup.set_channel(channel, &[0x10000 + channel as u32; 31], channel & 1 != 0); }
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    publish_channel_tables(&mut configuration, &setup).unwrap();
    for base in [0x401200, 0x4017eb, 0x401dd6, 0x402400, 0x4029eb, 0x402fd6] {
        for channel in 0..15 {
            let address = base + channel * 101;
            for knot in 0..31 {
                assert_eq!(configuration.byte(address + knot * 3), Some(channel as u8));
                assert_eq!(configuration.byte(address + knot * 3 + 2), Some(1));
            }
            assert_eq!(configuration.byte(address + 94), Some(16));
            assert_eq!(configuration.byte(address + 96), Some(16));
        }
    }
    assert_eq!(configuration.byte(0x4011ff), Some(0xa5));
    assert_eq!(configuration.byte(0x4035c1), Some(0xa5));
}

#[test]
fn clearing_setup_replaces_prior_channel_samples() {
    let mut bytes = std::vec![0; 0x420000];
    let mut setup = ChannelSetup::new();
    setup.set_channel(14, &[0x10000; 31], true);
    setup.clear();
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    publish_channel_tables(&mut configuration, &setup).unwrap();
    assert_eq!(configuration.byte(0x401200 + 14 * 101 + 2), Some(0));
}

#[test]
fn truncated_output_reports_capacity() {
    let mut bytes = std::vec![0; 0x401200];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    assert_eq!(publish_channel_tables(&mut configuration, &ChannelSetup::new()), Err(BackendError::Capacity));
}
