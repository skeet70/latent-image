use super::{ServiceMode, Station, StationList, StationSignal, WirelessActor, WirelessConfiguration, WirelessError};
use super::super::vendor_wireless_network::MacAddress;

fn configuration(seed: u8) -> WirelessConfiguration {
    let mut bytes = [seed; 32];
    bytes[..4].copy_from_slice(&1u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&8u32.to_le_bytes());
    WirelessConfiguration::from_abi(&bytes).unwrap()
}

#[test]
fn stored_configuration_preserves_all_abi_bytes() {
    let bytes = core::array::from_fn::<_, 32, _>(|index| index as u8);
    let configuration = WirelessConfiguration::from_abi(&bytes).unwrap();
    assert_eq!(configuration.as_abi(), bytes);
    let mut actor = WirelessActor::new(ServiceMode::Station, MacAddress([1, 2, 3, 4, 5, 6]));
    actor.set_configuration(configuration).unwrap();
    assert_eq!(actor.configuration().unwrap().as_abi(), bytes);
    assert_eq!(actor.copy_configuration(), MacAddress([1, 2, 3, 4, 5, 6]));
}

#[test]
fn failed_configuration_store_preserves_previous_configuration() {
    let mut actor = WirelessActor::new(ServiceMode::Station, MacAddress([0; 6]));
    actor.set_configuration(configuration(3)).unwrap();
    actor.set_cancelled(true);
    assert_eq!(actor.set_configuration(configuration(4)), Err(WirelessError::Cancelled));
    assert_eq!(actor.configuration(), Some(&configuration(3)));
    actor.set_cancelled(false);
    actor.set_configuration(configuration(5)).unwrap();
    assert_eq!(actor.configuration(), Some(&configuration(5)));
}

#[test]
fn wrong_mode_takes_precedence_over_cancellation() {
    for mode in [ServiceMode::Idle, ServiceMode::Transport, ServiceMode::RadioTest] {
        let mut actor = WirelessActor::new(mode, MacAddress([0; 6]));
        actor.set_cancelled(true);
        assert_eq!(actor.set_configuration(configuration(1)), Err(WirelessError::WrongMode));
        assert_eq!(actor.configuration(), None);
    }
}

#[test]
fn enumeration_preserves_reserved_bytes_and_trailing_output() {
    let actor = WirelessActor::new(ServiceMode::Idle, MacAddress([0; 6]));
    let stations = [
        Station { address: MacAddress([1, 2, 3, 4, 5, 6]), signal: StationSignal::from_observed_byte(100) },
        Station { address: MacAddress([11, 12, 13, 14, 15, 16]), signal: StationSignal::from_observed_byte(221) },
    ];
    let mut output = [0xa5; 29];
    assert_eq!(actor.enumerate_stations(StationList::Available(&stations), &mut output), Ok(2));
    assert_eq!(&output[..7], &[1, 2, 3, 4, 5, 6, 196]);
    assert_eq!(&output[7..12], &[0xa5; 5]);
    assert_eq!(&output[12..19], &[11, 12, 13, 14, 15, 16, 0]);
    assert_eq!(&output[19..], &[0xa5; 10]);
}

#[test]
fn station_signal_boundaries_match_exported_bytes() {
    let actor = WirelessActor::new(ServiceMode::Station, MacAddress([0; 6]));
    for (observed, expected) in [(0, 146), (1, 146), (2, 147), (219, 255), (220, 0), (221, 0), (255, 0)] {
        let stations = [Station { address: MacAddress([0; 6]), signal: StationSignal::from_observed_byte(observed) }];
        let mut output = [0; 12];
        actor.enumerate_stations(StationList::Available(&stations), &mut output).unwrap();
        assert_eq!(output[6], expected);
    }
}

#[test]
fn unavailable_or_insufficient_station_output_does_not_mutate_destination() {
    let actor = WirelessActor::new(ServiceMode::RadioTest, MacAddress([0; 6]));
    let mut output = [0xaa; 11];
    assert_eq!(actor.enumerate_stations(StationList::Unavailable, &mut output), Err(WirelessError::StationListUnavailable));
    let stations = [Station { address: MacAddress([0; 6]), signal: StationSignal::from_observed_byte(0) }];
    assert_eq!(actor.enumerate_stations(StationList::Available(&stations), &mut output), Err(WirelessError::OutputTooSmall));
    assert_eq!(output, [0xaa; 11]);
    assert_eq!(actor.enumerate_stations(StationList::Available(&[]), &mut output), Ok(0));
}

#[test]
fn configuration_boundary_requires_exact_extent() {
    assert_eq!(WirelessConfiguration::from_abi(&[0; 31]), Err(WirelessError::InvalidConfigurationExtent));
    assert_eq!(WirelessConfiguration::from_abi(&[0; 33]), Err(WirelessError::InvalidConfigurationExtent));
}

fn bss(flags: u32, address: [u8; 6]) -> super::super::vendor_wireless_network::Bss<'static> {
    use super::super::vendor_wireless_network::{Bss, BssFlags, Capabilities, FrequencyMhz, SignalDbm, Ssid};
    Bss::new(BssFlags(flags), MacAddress(address), Ssid::parse(b"network").unwrap(), FrequencyMhz(2412), Capabilities(0x11), SignalDbm(-42), &[]).unwrap()
}

#[test]
fn scan_publication_uses_owned_network_rows_and_preserves_empty_list_count() {
    use super::super::vendor_wireless_network::{BssFlags, ScanRow};
    let mut actor = WirelessActor::new(ServiceMode::Station, MacAddress([0; 6]));
    let mut rows = [ScanRow::empty(); 64];
    rows[1].flags = BssFlags(99);
    let entries = [bss(77, [1, 2, 3, 4, 5, 6])];
    assert_eq!(actor.export_scan_rows(Some(&entries), &mut rows).unwrap().unwrap().get(), 1);
    assert_eq!(rows[0].address, MacAddress([1, 2, 3, 4, 5, 6]));
    assert_eq!(rows[0].signal.0, -42);
    assert_eq!(rows[0].flags, BssFlags(77));
    assert_eq!(rows[1].flags, BssFlags(0));
    assert_eq!(actor.export_scan_rows(None, &mut rows), Ok(None));
    assert_eq!(rows[0].flags, BssFlags(77));
    assert_eq!(actor.export_scan_rows(Some(&[]), &mut rows).unwrap().unwrap().get(), 1);
    assert_eq!(rows[0].flags, BssFlags(0));
    assert_eq!(rows[0].address, MacAddress([1, 2, 3, 4, 5, 6]));
}

#[test]
fn scan_publication_exports_at_most_sixty_four_rows() {
    use super::super::vendor_wireless_network::ScanRow;
    let mut actor = WirelessActor::new(ServiceMode::Station, MacAddress([0; 6]));
    let entries: std::vec::Vec<_> = (0..70).map(|index| bss(index, [index as u8; 6])).collect();
    let mut rows = [ScanRow::empty(); 64];
    assert_eq!(actor.export_scan_rows(Some(&entries), &mut rows).unwrap().unwrap().get(), 64);
    assert_eq!(rows[63].address, MacAddress([63; 6]));
}

#[test]
fn scan_guards_precede_absent_supplicant_and_preserve_rows() {
    use super::super::vendor_wireless_network::{BssFlags, ScanRow};
    let mut rows = [ScanRow::empty(); 64];
    rows[0].flags = BssFlags(99);
    let mut actor = WirelessActor::new(ServiceMode::Idle, MacAddress([0; 6]));
    actor.set_cancelled(true);
    assert_eq!(actor.export_scan_rows(None, &mut rows), Err(WirelessError::WrongMode));
    let mut actor = WirelessActor::new(ServiceMode::Station, MacAddress([0; 6]));
    actor.set_cancelled(true);
    assert_eq!(actor.export_scan_rows(None, &mut rows), Err(WirelessError::Cancelled));
    assert_eq!(rows[0].flags, BssFlags(99));
}

#[test]
fn completion_codes_publish_the_actual_connection_state() {
    let mut actor = WirelessActor::new(ServiceMode::Station, MacAddress([0; 6]));
    for (code, expected) in [
        (0, 1), (1, 2), (11, 12), (12, 0), (-1, 0),
        (i32::MIN, 13), (i32::MIN + 1, 14), (i32::MIN + 7, 20), (i32::MIN + 8, 0),
    ] {
        actor.apply_completion_code(code);
        assert_eq!(actor.connection_completion().get(), expected);
    }
}
