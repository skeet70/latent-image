use super::*;

fn ht_beacon(wide: bool) -> [u8; 28] {
    let mut bytes = [0; 28];
    bytes[0] = 45;
    bytes[1] = 26;
    bytes[2] = if wide { 0x62 } else { 0x20 };
    bytes[3] = 0x10;
    bytes[4] = 3;
    bytes[5] = 0xff;
    bytes[9] = u8::from(wide);
    bytes
}

#[test]
fn legacy_peer_receives_no_ht_or_wmm() {
    let mut elements = AssociationElements::new();
    elements.observe_beacon(&[3, 1, 6]).unwrap();
    let mut packet = Packet::new();
    elements.append_before_security(&mut packet).unwrap();
    elements.append_after_security(&mut packet).unwrap();
    assert_eq!(packet.as_bytes(), &[]);
}

#[test]
fn ht_twenty_mhz_request_has_owned_capability_and_mcs_bytes() {
    let mut elements = AssociationElements::new();
    elements.observe_beacon(&ht_beacon(true)).unwrap();
    let mut packet = Packet::new();
    elements.append_before_security(&mut packet).unwrap();
    elements.append_after_security(&mut packet).unwrap();
    assert_eq!(packet.as_bytes(), &[45,26,0x20,0,3,0xff,0,0,0,0,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0,0,0,0,127,1,0]);
}

#[test]
fn forty_mhz_and_short_guard_interval_configuration_changes_wire_contract() {
    let mut elements = AssociationElements::new();
    elements.configure(ElementConfiguration { width: ChannelWidth::Forty, disable_short_guard_interval: true, ..ElementConfiguration::new() });
    elements.observe_beacon(&ht_beacon(true)).unwrap();
    let mut packet = Packet::new();
    elements.append_before_security(&mut packet).unwrap();
    assert_eq!(&packet.as_bytes()[..6], &[45,26,2,0x10,3,0xff]);
    assert_eq!(packet.as_bytes()[9], 1);
    assert_eq!(packet.as_bytes()[21], 1);
}

#[test]
fn wmm_request_intersects_delivery_and_trigger_access_categories() {
    let mut elements = AssociationElements::new();
    elements.configure(ElementConfiguration { delivery_enabled: AccessCategories::ALL,
        trigger_enabled: AccessCategories::new(true, false, false, true),
        service_period: ServicePeriod::Four, ..ElementConfiguration::new() });
    elements.observe_beacon(&[221,7,0,0x50,0xf2,2,0,1,0x80]).unwrap();
    let mut packet = Packet::new();
    elements.append_after_security(&mut packet).unwrap();
    assert_eq!(packet.as_bytes(), &[221,7,0,0x50,0xf2,2,0,1,0x49]);
}

#[test]
fn response_negotiates_ht_operation_and_coexistence_timer() {
    let mut elements = AssociationElements::new();
    elements.configure(ElementConfiguration { width: ChannelWidth::Forty, coexistence_scan_enabled: true, ..ElementConfiguration::new() });
    elements.observe_beacon(&ht_beacon(true)).unwrap();
    let mut bytes = [0; 40];
    bytes[..8].copy_from_slice(&[61,22,6,5,7,0,0x34,0x12]);
    bytes[24..26].copy_from_slice(&[74,14]);
    bytes[30] = 4;
    assert_eq!(elements.observe_response(&bytes), Ok(TimerChange::Arm(Milliseconds(10_000))));
    let state = elements.negotiated();
    assert_eq!(state.channel, Some(6));
    assert!(state.forty_mhz);
    let operation = state.operation.unwrap();
    assert_eq!(operation.secondary_channel, SecondaryChannel::Above);
    assert_eq!(operation.protection, 7);
    assert_eq!(operation.basic_stbc, 0x1234);
    assert_eq!(elements.observe_response(&bytes), Ok(TimerChange::Unchanged));
    assert_eq!(elements.cancel(), TimerChange::Cancel);
    assert_eq!(elements.negotiated().ht, None);
}

#[test]
fn malformed_response_preserves_negotiation_and_timer() {
    let mut elements = AssociationElements::new();
    elements.observe_beacon(&ht_beacon(true)).unwrap();
    let previous = elements.negotiated();
    assert_eq!(elements.observe_response(&[3,1,6,45,26,0]), Err(ElementError::Truncated));
    assert_eq!(elements.negotiated(), previous);
    assert_eq!(elements.observe_response(&[3,1,200]), Err(ElementError::InvalidChannel));
    assert_eq!(elements.negotiated(), previous);
}

#[test]
fn rsn_serializes_selected_suites_and_cached_pmkid() {
    let mut elements = AssociationElements::new();
    elements.configure(ElementConfiguration { security: SecurityElements::Rsn {
        suites: SecuritySuites { group: Cipher::Tkip, pairwise: Cipher::Ccmp, key_management: KeyManagement::PreSharedKey },
        pmkid: Some(Pmkid([0x35;16])) }, ..ElementConfiguration::new() });
    let mut packet = Packet::new();
    elements.append_after_security(&mut packet).unwrap();
    assert_eq!(&packet.as_bytes()[..24], &[48,38,1,0,0,0x0f,0xac,2,1,0,0,0x0f,0xac,4,1,0,0,0x0f,0xac,2,0,0,1,0]);
    assert_eq!(&packet.as_bytes()[24..], &[0x35;16]);
}

#[test]
fn wpa_is_distinct_from_wmm_and_rsn() {
    let mut elements = AssociationElements::new();
    elements.configure(ElementConfiguration { security: SecurityElements::Wpa(SecuritySuites {
        group: Cipher::Tkip, pairwise: Cipher::Tkip, key_management: KeyManagement::Enterprise }), ..ElementConfiguration::new() });
    let mut packet = Packet::new();
    elements.append_after_security(&mut packet).unwrap();
    assert_eq!(packet.as_bytes(), &[221,22,0,0x50,0xf2,1,1,0,0,0x50,0xf2,2,1,0,0,0x50,0xf2,2,1,0,0,0x50,0xf2,1]);
}

#[test]
fn response_decodes_wmm_contention_and_txop_values() {
    let mut elements = AssociationElements::new();
    elements.observe_beacon(&[221,7,0,0x50,0xf2,2,0,1,0x80]).unwrap();
    let bytes = [221,24,0,0x50,0xf2,2,1,1,3,0,0x13,0xa4,0x34,0x12,0x27,0x43,0,0,0x42,0x32,94,0,0x62,0x32,47,0];
    elements.observe_response(&bytes).unwrap();
    let wmm = elements.negotiated().wmm.unwrap();
    assert!(wmm.uapsd);
    assert_eq!(wmm.parameter_set, 3);
    let ac = wmm.access_categories.unwrap();
    assert_eq!(ac[0], AccessCategoryParameters { aifsn: 3, admission_required: true, contention_min: 15, contention_max: 1023, txop: 0x1234 });
    assert_eq!(ac[2].txop, 94);
    assert_eq!(ac[3].txop, 47);
}

#[test]
fn radio_parameters_project_ht_and_erp_negotiation_for_the_retained_command() {
    let mut elements = AssociationElements::new();
    elements.configure(ElementConfiguration { width: ChannelWidth::Forty, ..ElementConfiguration::new() });
    elements.observe_beacon(&ht_beacon(true)).unwrap();
    elements.set_peer_capabilities(0x420);
    let mut bytes = [0; 27];
    bytes[..8].copy_from_slice(&[61,22,6,5,7,0,0x34,0x12]);
    bytes[24..].copy_from_slice(&[42,1,6]);
    elements.observe_response(&bytes).unwrap();
    let radio = elements.radio_parameters(true);
    assert_eq!(radio.secondary_channel, SecondaryChannel::Above);
    assert_eq!(radio.ht_protection, super::super::ibss::HtProtection::NonHtMixed);
    assert!(radio.erp_protection);
    assert!(radio.non_greenfield_present);
    assert!(radio.rifs_protection);
    assert_eq!(radio.basic_stbc, 0x1234);
    assert_eq!(radio.operation_protection, 7);
    assert_eq!(radio.operation_information, 5);
    assert!(!radio.short_preamble);
    assert!(radio.short_slot);
}

#[test]
fn supported_and_extended_rates_use_the_target_bit_positions_and_basic_flags() {
    let rates = RateSet::parse(6, &[1,8,0x82,0x84,0x8b,0x96,44,66,12,18,50,6,24,36,48,72,96,108]).unwrap();
    assert_eq!(rates.operational_rates.wire(), 0x3fff);
    assert_eq!(rates.basic_rates.wire(), 15);
    let negotiated = rates.negotiate(PhySet::STOCK, RateMask::LEGACY).unwrap();
    assert_eq!(negotiated.desired_rates.wire(), 0x3fcf);
    assert_eq!(negotiated.basic_rates.wire(), 15);
    assert_eq!(negotiated.desired_phy.wire(), 3);
}

#[test]
fn unknown_optional_rates_are_ignored_but_unknown_basic_rates_refuse_negotiation() {
    let optional = RateSet::from_rates(6, &[0x82,9]).unwrap();
    assert_eq!(optional.operational_rates.wire(), 1);
    assert!(optional.negotiate(PhySet::STOCK, RateMask::LEGACY).is_ok());
    let required = RateSet::from_rates(6, &[0x82,0x89]).unwrap();
    assert_eq!(required.negotiate(PhySet::STOCK, RateMask::LEGACY), Err(ElementError::UnsupportedBasicRate));
}

#[test]
fn basic_ofdm_suppresses_cck_phy_and_negotiation_intersects_local_capabilities() {
    let rates = RateSet::parse(6, &[1,3,2,4,0x8c,45,0]).unwrap();
    assert_eq!(rates.peer_phy.wire(), 0x12);
    let negotiated = rates.negotiate(PhySet::STOCK.without_ht(), RateMask::LEGACY).unwrap();
    assert_eq!(negotiated.desired_phy.wire(), 2);
    assert_eq!(negotiated.desired_rates.wire(), 0x43);
    assert_eq!(negotiated.basic_rates.wire(), 0x40);
}

#[test]
fn target_rate_sentinel_disables_legacy_phy_without_discarding_ht() {
    let rates = RateSet::parse(6, &[1,2,0x82,0xff,45,0]).unwrap();
    assert_eq!(rates.basic_rates.wire(), 0x4001);
    assert_eq!(rates.peer_phy.wire(), 0x10);
    assert_eq!(rates.negotiate(PhySet::STOCK, RateMask::LEGACY).unwrap().desired_phy.wire(), 0x10);
    assert_eq!(rates.negotiate(PhySet::STOCK.without_ht(), RateMask::LEGACY), Err(ElementError::NoCommonPhy));
}

#[test]
fn five_ghz_uses_its_phy_flag_and_rejects_cck_rates() {
    let rates = RateSet::from_rates(36, &[0x8c,0x98,0xb0]).unwrap();
    assert_eq!(rates.peer_phy.wire(), 8);
    assert_eq!(rates.negotiate(PhySet::STOCK, RateMask::LEGACY), Err(ElementError::NoCommonPhy));
    let local = PhySet::from_bits(8).unwrap();
    assert_eq!(rates.negotiate(local, RateMask::LEGACY).unwrap().desired_rates.wire(), 0x540);
    assert_eq!(RateSet::from_rates(36, &[0x82,12]), Err(ElementError::InvalidRateBand));
}

#[test]
fn duplicate_rate_elements_retain_first_occurrence_and_reject_truncation() {
    let rates = RateSet::parse(6, &[1,1,0x82,1,1,0x84,50,1,12,50,1,18]).unwrap();
    assert_eq!(rates.operational_rates.wire(), 0x41);
    assert_eq!(rates.basic_rates.wire(), 1);
    assert_eq!(RateSet::parse(6, &[1,2,2]), Err(ElementError::Truncated));
}
