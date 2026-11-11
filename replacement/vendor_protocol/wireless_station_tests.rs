use super::{Cipher, DriverAction, DriverCommand, DriverStatus, InformationElements, KeyMaterial, PersonalProtocol, ProfileError, ScanBatch, ScanCandidate, SelectedAccessPoint, StationError, StationOutcome, StationProfile, StationRequest, StationRuntime, StationState, WpsConfiguration, WpsMethod, WpsPin};
use super::super::super::vendor_wireless_network::{MacAddress, Ssid};

fn profile(security: u8, key: &[u8]) -> StationProfile { StationProfile::parse(Ssid::parse(b"synthetic").unwrap(), security, key, 0).unwrap() }
fn candidate(security: Cipher) -> ScanCandidate {
    ScanCandidate { ssid: Ssid::parse(b"synthetic").unwrap(), privacy: security != Cipher::None, signal: super::super::super::vendor_wireless_network::SignalDbm(-30), mode: super::super::super::vendor_wireless_network::NetworkMode::Infrastructure, key_management: super::super::super::vendor_wireless_network::KeyManagement::Unspecified, access_point: SelectedAccessPoint { address: MacAddress([2,0,0,0,0,1]), protocol: PersonalProtocol::Rsn, pairwise: security, group: security, advertised_elements: InformationElements::parse(&[]).unwrap() } }
}
fn acknowledge(runtime: &mut StationRuntime, command: DriverCommand) { runtime.receive(command.acknowledge(Ok(())).unwrap()).unwrap(); }
fn scan_command(runtime: &mut StationRuntime) -> DriverCommand {
    loop { let command = runtime.poll().expect("pending station request"); if matches!(command.action(), DriverAction::Scan { .. }) { return command; } acknowledge(runtime, command); }
}
fn publish_candidate(runtime: &mut StationRuntime, command: DriverCommand, candidate: ScanCandidate) {
    let mut results = ScanBatch::new(); results.append(candidate).unwrap(); runtime.receive(command.scanned(results).unwrap()).unwrap();
}

#[test]
fn credentials_reject_invalid_keys_before_starting_any_transaction() {
    let ssid = Ssid::parse(b"synthetic").unwrap();
    assert_eq!(StationProfile::parse(ssid, 1, b"12345", 4), Err(ProfileError::KeyIndex));
    assert_eq!(StationProfile::parse(ssid, 2, b"short", 0), Err(ProfileError::KeyLength));
    assert_eq!(StationProfile::parse(ssid, 3, &[b'z'; 64], 0), Err(ProfileError::Hex));
    assert_eq!(StationProfile::parse(ssid, 99, &[], 0), Err(ProfileError::Security));
    let ascii = StationProfile::parse(ssid, 1, b"abcde", 2).unwrap();
    let hex = StationProfile::parse(ssid, 1, b"6162636465", 2).unwrap();
    assert_eq!(ascii, hex);
}

#[test]
fn open_connection_waits_for_matching_association_indication() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    let operation = runtime.submit(StationRequest::Select(profile(0, &[]))).unwrap();
    assert!(runtime.completion().is_none());
    let command = scan_command(&mut runtime);
    let peer = candidate(Cipher::None);
    publish_candidate(&mut runtime, command, peer.clone());
    while let Some(command) = runtime.poll() { acknowledge(&mut runtime, command); }
    assert_eq!(runtime.state(), StationState::Associating);
    assert!(runtime.completion().is_none());
    assert!(matches!(runtime.associated(operation, MacAddress([7; 6])), Err(StationError::Handshake(_))));
    runtime.associated(operation, peer.access_point.address).unwrap();
    let completion = runtime.completion().unwrap();
    assert_eq!(completion.operation(), operation);
    assert_eq!(completion.outcome(), StationOutcome::Connected(peer.access_point.address));
}

#[test]
fn cancellation_waits_for_inflight_key_and_removes_it_before_completion() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    let operation = runtime.submit(StationRequest::Select(profile(1, b"abcde"))).unwrap();
    let scan = scan_command(&mut runtime);
    publish_candidate(&mut runtime, scan, candidate(Cipher::Wep40));
    let key_command = loop {
        let command = runtime.poll().unwrap();
        if let DriverAction::InstallKey(key) = command.action() { assert!(matches!(&key.material, KeyMaterial::Wep(key) if key.as_bytes() == b"abcde")); break command; }
        acknowledge(&mut runtime, command);
    };
    runtime.cancel(operation).unwrap();
    assert!(runtime.poll().is_none());
    assert!(runtime.completion().is_none());
    assert_eq!(runtime.submit(StationRequest::Scan), Err(StationError::Busy));
    acknowledge(&mut runtime, key_command);
    let removal = runtime.poll().unwrap();
    assert!(matches!(removal.action(), DriverAction::RemoveKey(_)));
    acknowledge(&mut runtime, removal);
    assert!(runtime.completion().is_none());
    let disconnect = runtime.poll().unwrap();
    assert!(matches!(disconnect.action(), DriverAction::Disconnect));
    acknowledge(&mut runtime, disconnect);
    assert_eq!(runtime.completion().unwrap().outcome(), StationOutcome::Cancelled);
}

#[test]
fn stale_association_cannot_complete_new_selection() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    let first = runtime.submit(StationRequest::Select(profile(0, &[]))).unwrap();
    runtime.cancel(first).unwrap();
    while let Some(command) = runtime.poll() { acknowledge(&mut runtime, command); }
    let second = runtime.submit(StationRequest::SelectDefault).unwrap();
    assert_ne!(first, second);
    assert_eq!(runtime.associated(first, MacAddress([2,0,0,0,0,1])), Err(StationError::StaleReply));
    assert!(runtime.completion().is_none());
    assert_eq!(runtime.state(), StationState::Selecting);
}

#[test]
fn failed_driver_control_never_publishes_connection_success() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    runtime.submit(StationRequest::Select(profile(0, &[]))).unwrap();
    let command = runtime.poll().unwrap();
    let failure = DriverStatus(0xc0010015);
    assert_eq!(runtime.receive(command.acknowledge(Err(failure)).unwrap()), Err(StationError::Driver(failure)));
    assert!(runtime.completion().is_none());
    while let Some(command) = runtime.poll() { acknowledge(&mut runtime, command); }
    assert_eq!(runtime.completion().unwrap().outcome(), StationOutcome::Failed(StationError::Driver(failure)));
}

#[test]
fn wps_cancellation_discards_entropy_reply_before_any_credentials_are_applied() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    let operation = runtime.submit(StationRequest::ApplyWps(WpsConfiguration { method: WpsMethod::Pbc, bssid: None })).unwrap();
    let entropy = loop { let command = runtime.poll().unwrap(); if matches!(command.action(), DriverAction::RandomNonce) { break command; } acknowledge(&mut runtime, command); };
    assert_eq!(runtime.submit(StationRequest::CancelWps), Ok(operation));
    assert!(runtime.poll().is_none());
    runtime.receive(entropy.nonce([42; 32]).unwrap()).unwrap();
    while let Some(command) = runtime.poll() { acknowledge(&mut runtime, command); }
    assert!(runtime.wps_credential().is_none());
    assert!(runtime.profile().is_none());
    assert_eq!(runtime.completion().unwrap().outcome(), StationOutcome::Cancelled);
}

#[test]
fn generated_pin_keeps_leading_zeroes_and_validates_checksum() {
    let pin = WpsPin::generate(12_345u32.to_le_bytes());
    assert_eq!(pin.as_bytes(), b"00123457");
    assert_eq!(WpsPin::parse(pin.as_bytes()), Ok(pin));
    assert!(WpsPin::parse(b"00123458").is_err());
    assert!(WpsPin::parse(b"1234").is_ok());
    assert!(WpsPin::parse(b"12a4").is_err());
}

#[test]
fn shutdown_receipt_requires_drained_disconnect_and_disables_new_requests() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    assert!(matches!(runtime.finish_shutdown(), Err(StationError::WrongReply)));
    let operation = runtime.submit(StationRequest::Leave).unwrap();
    assert!(matches!(runtime.finish_shutdown(), Err(StationError::Busy)));
    while let Some(command) = runtime.poll() { acknowledge(&mut runtime, command); }
    let stopped = runtime.finish_shutdown().unwrap();
    assert_eq!(stopped.operation(), operation);
    assert_eq!(runtime.state(), StationState::Disabled);
    assert_eq!(runtime.submit(StationRequest::Scan), Err(StationError::Disabled));
    assert!(matches!(runtime.finish_shutdown(), Err(StationError::Busy)));
}

#[test]
fn prefix_scan_filters_by_the_source_eight_byte_prefix() {
    let mut runtime = StationRuntime::new(MacAddress([2; 6]));
    let operation = runtime.submit(StationRequest::ScanPrefix(Ssid::parse(b"synthetic suffix").unwrap())).unwrap();
    let command = scan_command(&mut runtime);
    let mut results = ScanBatch::new();
    results.append(candidate(Cipher::None)).unwrap();
    let mut other = candidate(Cipher::None); other.ssid = Ssid::parse(b"other").unwrap(); results.append(other).unwrap();
    let mut same_prefix = candidate(Cipher::None); same_prefix.ssid = Ssid::parse(b"syntheti different").unwrap(); results.append(same_prefix).unwrap();
    runtime.receive(command.scanned(results).unwrap()).unwrap();
    assert_eq!(runtime.completion().unwrap().operation(), operation);
    assert_eq!(runtime.completion().unwrap().outcome(), StationOutcome::ScanComplete);
    let names: std::vec::Vec<_> = runtime.scan_results().candidates().map(|candidate| candidate.ssid).collect();
    assert_eq!(names, [Ssid::parse(b"synthetic").unwrap(), Ssid::parse(b"syntheti different").unwrap()]);
}
