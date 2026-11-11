use super::*;

fn slot(pairwise: bool) -> StationKeySlot {
    StationKeySlot { index: KeyIndex::new(0).unwrap(), peer: MacAddress([2, 3, 4, 5, 6, 7]), pairwise }
}
fn key(material: KeyMaterial) -> StationKey {
    StationKey { slot: slot(true), transmit: true, authenticator: false, material }
}
fn finish(driver: &mut StationDriver) {
    let command = driver.command().unwrap();
    let identity = command.identity;
    assert_eq!(driver.receive(identity, SdioWriteOutcome::Written), StationReply::Applied);
}

#[test]
fn pairwise_ccmp_has_concrete_command_fifo_bytes() {
    let mut driver = StationDriver::new(0);
    driver.submit(StationDriverRequest::SetAuthentication(Authentication::Wpa2Psk)).unwrap();
    driver.submit(StationDriverRequest::SetEncryption(Encryption::Ccmp)).unwrap();
    driver.submit(StationDriverRequest::InstallKey(key(KeyMaterial::Temporal128([0xa5; 16])))).unwrap();
    let command = driver.command().unwrap();
    assert_eq!(command.argument(), 0x9000_5848);
    assert_eq!(&command.bytes()[..8], &[72, 0, 0, 0x50, 8, 1, 1, 0]);
    assert_eq!(&command.bytes()[8..24], &[1, 1, 1, 0, 2, 3, 4, 5, 6, 7, 0, 4, 0, 16, 0, 0]);
    assert_eq!(&command.bytes()[24..40], &[0xa5; 16]);
    assert_eq!(&command.bytes()[40..], &[0; 32]);
    assert!(driver.security().transmit_key_present);
}

#[test]
fn wep_lengths_select_distinct_cipher_ids() {
    for (material, cipher, length) in [(KeyMaterial::Wep40([9; 5]), 1, 5), (KeyMaterial::Wep104([9; 13]), 5, 13)] {
        let mut driver = StationDriver::new(0);
        driver.submit(StationDriverRequest::InstallKey(StationKey { slot: slot(false), transmit: true, authenticator: false, material })).unwrap();
        let bytes = driver.command().unwrap().bytes();
        assert_eq!(bytes[19], cipher);
        assert_eq!(bytes[21], length);
        assert_eq!(&bytes[24..24 + length as usize], &[9; 13][..length as usize]);
    }
}

#[test]
fn unauthenticated_temporal128_uses_cipher_seven() {
    let mut driver = StationDriver::new(0);
    driver.submit(StationDriverRequest::InstallKey(key(KeyMaterial::Temporal128([4; 16])))).unwrap();
    assert_eq!(driver.command().unwrap().bytes()[19], 7);
}

#[test]
fn pairwise_key_validation_does_not_emit_physical_request() {
    let mut driver = StationDriver::new(0);
    let mut invalid = key(KeyMaterial::Temporal128([1; 16]));
    invalid.slot.peer = MacAddress([255; 6]);
    assert_eq!(driver.submit(StationDriverRequest::InstallKey(invalid)), Err(StationDriverError::InvalidKey));
    assert!(driver.command().is_none());
    invalid.slot.peer = MacAddress([2, 3, 4, 5, 6, 7]);
    invalid.transmit = false;
    assert_eq!(driver.submit(StationDriverRequest::InstallKey(invalid)), Err(StationDriverError::InvalidKey));
    assert!(driver.command().is_none());
}

#[test]
fn no_response_selector_completes_in_owned_transmit_continuation() {
    let mut driver = StationDriver::new(255);
    driver.submit(StationDriverRequest::InstallKey(key(KeyMaterial::Temporal128([2; 16])))).unwrap();
    let command = driver.command().unwrap();
    let identity = command.identity;
    assert_eq!(command.sequence(), 0);
    assert_eq!(driver.receive(identity, SdioWriteOutcome::Failed), StationReply::Applied);
    assert_eq!(driver.last_physical_outcome(), Some(SdioWriteOutcome::Failed));
    assert_eq!(driver.outcome(), Some(Ok(())));
    assert_eq!(driver.receive(identity, SdioWriteOutcome::Written), StationReply::Late);
}

#[test]
fn pending_selector_serializes_until_transmit_continuation() {
    let mut driver = StationDriver::new(10);
    driver.submit(StationDriverRequest::InstallKey(key(KeyMaterial::Temporal128([2; 16])))).unwrap();
    assert_eq!(driver.submit(StationDriverRequest::SetEncryption(Encryption::Ccmp)), Err(StationDriverError::Busy));
    finish(&mut driver);
    assert_eq!(driver.outcome(), Some(Ok(())));
    driver.submit(StationDriverRequest::SetEncryption(Encryption::Ccmp)).unwrap();
    assert_eq!(driver.security().encryption, Encryption::Ccmp);
}

#[test]
fn removal_emits_zeroed_key_material_and_clears_pairwise_transmit_flag() {
    let mut driver = StationDriver::new(0);
    driver.submit(StationDriverRequest::InstallKey(key(KeyMaterial::Temporal128([3; 16])))).unwrap();
    finish(&mut driver);
    driver.submit(StationDriverRequest::RemoveKey(slot(true))).unwrap();
    let bytes = driver.command().unwrap().bytes();
    assert_eq!(&bytes[8..12], &[0, 0, 1, 0]);
    assert_eq!(&bytes[12..18], &[2, 3, 4, 5, 6, 7]);
    assert_eq!(&bytes[18..], &[0; 54]);
    assert!(!driver.security().transmit_key_present);
}

#[test]
fn stop_guard_rejects_without_command() {
    let mut driver = StationDriver::new(0);
    driver.set_stopped(true);
    assert_eq!(driver.submit(StationDriverRequest::SetAuthentication(Authentication::Open)), Err(StationDriverError::Stopped));
    assert!(driver.command().is_none());
}

#[test]
fn cancellation_drops_late_transmit_completion() {
    let mut driver = StationDriver::new(0);
    driver.submit(StationDriverRequest::InstallKey(key(KeyMaterial::Temporal128([3; 16])))).unwrap();
    let command = driver.command().unwrap();
    let identity = command.identity;
    driver.cancel();
    assert_eq!(driver.receive(identity, SdioWriteOutcome::Written), StationReply::Late);
    assert_eq!(driver.outcome(), Some(Err(StationDriverError::Cancelled)));
}

#[test]
fn immediate_selectors_publish_distinct_operation_identities() {
    let mut driver = StationDriver::new(0);
    let first = driver.submit_identified(StationDriverRequest::SetAuthentication(Authentication::Open)).unwrap();
    let completed = driver.completion().unwrap();
    assert_eq!(completed.operation(), first);
    assert_eq!(completed.outcome(), Ok(()));
    let second = driver.submit_identified(StationDriverRequest::SetEncryption(Encryption::Ccmp)).unwrap();
    assert_ne!(first, second);
    assert_eq!(driver.completion().unwrap().operation(), second);
}

#[test]
fn cipher_selection_exposes_negotiated_protocol_suites() {
    let mut driver = StationDriver::new(0);
    driver.submit(StationDriverRequest::SetAuthentication(Authentication::Wpa2Psk)).unwrap();
    driver.submit(StationDriverRequest::SetEncryption(Encryption::Ccmp)).unwrap();
    assert_eq!(driver.security().authentication_suite(), Some(AuthenticationSuite::RsnPersonal));
    assert_eq!(driver.security().cipher_suites(), CipherSuites { wep40: true, wep104: true, tkip: true, ccmp: true });
    driver.submit(StationDriverRequest::SetEncryption(Encryption::Disabled)).unwrap();
    assert_eq!(driver.security().cipher_suites(), CipherSuites { wep40: true, wep104: true, tkip: false, ccmp: false });
}

#[test]
fn authentication_mode_constraints_are_enforced_before_effects() {
    let mut driver = StationDriver::new(0);
    assert_eq!(driver.submit(StationDriverRequest::SetAuthentication(Authentication::WpaNone)), Err(StationDriverError::InvalidAuthenticationMode));
    assert_eq!(driver.security().authentication, Authentication::Open);
    assert!(driver.command().is_none());
}

#[test]
fn cancellation_invalidates_association_scan_continuations() {
    let mut driver = StationDriver::new(0);
    driver.submit(StationDriverRequest::SelectSsid(Ssid::parse(b"camera").unwrap())).unwrap();
    let old_command = driver.association().command().unwrap().token();
    driver.cancel();
    assert_eq!(driver.association().state(), association::AssociationState::Idle);
    assert_eq!(driver.association_mut().finish_command(old_command, Ok(())), Err(association::AssociationError::StaleCompletion));
    assert_eq!(driver.outcome(), Some(Err(StationDriverError::Cancelled)));
}
