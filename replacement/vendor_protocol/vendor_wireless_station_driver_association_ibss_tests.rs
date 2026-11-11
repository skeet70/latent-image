use super::*;
use super::super::super::super::vendor_wireless_network::SignalDbm;

const LOCAL: MacAddress = MacAddress([2, 3, 4, 5, 6, 7]);

fn create(engine: &mut Ibss) {
    engine.create(InterfaceMode::Adhoc, LOCAL, Ssid::parse(b"camera").unwrap(),
        CreationParameters::new(Band::Ghz2, 6), Entropy([0x0101, 0x2233, 0x4455])).unwrap();
}
fn finish(engine: &mut Ibss) -> Vec<IbssCommand> {
    let mut result = Vec::new();
    for sequence in 0..32 {
        let Some(command) = engine.command(sequence).unwrap() else { return result; };
        result.push(command);
        engine.complete(Ok(())).unwrap();
    }
    panic!("IBSS did not reach an external wait");
}
fn candidate(capabilities: u16) -> Candidate {
    let mut beacon = vec![0; 36];
    beacon[0] = 0x80;
    beacon[4..10].fill(0xff);
    beacon[10..16].copy_from_slice(&[2, 9, 8, 7, 6, 5]);
    beacon[16..22].copy_from_slice(&[2, 8, 7, 6, 5, 4]);
    beacon[32..34].copy_from_slice(&200u16.to_le_bytes());
    beacon[34..36].copy_from_slice(&capabilities.to_le_bytes());
    beacon.extend_from_slice(&[0, 6, b'c', b'a', b'm', b'e', b'r', b'a', 1, 4, 0x82, 0x84, 0x8b, 0x96, 3, 1, 6]);
    Candidate::parse_beacon(&beacon, SignalDbm(-32)).unwrap()
}

#[test]
fn creation_programs_target_bss_and_only_beacon_information_elements() {
    let mut engine = Ibss::new();
    create(&mut engine);
    let commands = finish(&mut engine);
    let codes: Vec<_> = commands.iter().filter(|command| command.kind == IbssCommandKind::Firmware)
        .map(|command| command.packet.as_bytes()[4]).collect();
    assert_eq!(codes, [0x16, 0x1c, 0x19]);
    let bss = commands[0].packet.as_bytes();
    assert_eq!(&bss[8..12], &[0, 0, 1, 6]);
    assert_eq!(&bss[12..18], b"camera");
    assert_eq!(&bss[44..50], &[2, 1, 0x33, 0x22, 0x55, 0x44]);
    assert_eq!(&bss[52..56], &[0xcf, 0x3f, 0x0f, 0]);
    assert_eq!(&bss[63..69], &LOCAL.0);
    let beacon = commands[1].packet.as_bytes();
    assert_eq!(&beacon[8..16], &[1, 0, 0, 0, 2, 0, 31, 0]);
    assert_eq!(&beacon[16..], &[0, 6, b'c', b'a', b'm', b'e', b'r', b'a',
        1, 8, 0x82, 0x84, 0x8b, 0x96, 12, 18, 24, 36, 3, 1, 6, 6, 2, 0, 0,
        50, 4, 48, 72, 96, 108]);
    assert_eq!(commands.last().unwrap().kind, IbssCommandKind::ArmAloneTimer);
    assert!(!commands.iter().any(|command| command.kind == IbssCommandKind::Publish));
    assert!(engine.active());
}

#[test]
fn five_ghz_creation_uses_target_basic_rates_without_ds_element() {
    let mut engine = Ibss::new();
    engine.create(InterfaceMode::Dedicated, LOCAL, Ssid::parse(b"a").unwrap(),
        CreationParameters::new(Band::Ghz5, 36), Entropy([0xffff, 0, 0])).unwrap();
    let commands = finish(&mut engine);
    assert_eq!(&commands[0].packet.as_bytes()[52..56], &[0xc0, 0x3f, 0x40, 5]);
    assert_eq!(&commands[1].packet.as_bytes()[16..], &[0, 1, b'a', 1, 8, 0x8c, 18, 0x98, 36, 0xb0, 72, 96, 108, 6, 2, 0, 0]);
    assert_eq!(engine.bssid(), Some(MacAddress([0xfe, 0xff, 0, 0, 0, 0])));
}

#[test]
fn merge_cancels_alone_timer_and_publishes_only_after_peer_acceptance() {
    let mut engine = Ibss::new(); create(&mut engine); finish(&mut engine);
    engine.set_peer(PeerIndex::new(7).unwrap(), PeerParameters::legacy()).unwrap();
    engine.merge(&candidate(0x432), LOCAL).unwrap();
    let commands = finish(&mut engine);
    assert_eq!(commands[0].kind, IbssCommandKind::CancelAloneTimer);
    let peer: Vec<_> = commands.iter().filter(|command| command.kind == IbssCommandKind::Firmware
        && command.packet.as_bytes()[4] == 0x17).collect();
    assert_eq!(peer.len(), 2);
    assert_eq!(&peer[0].packet.as_bytes()[8..16], &[7, 0x11, 2, 9, 8, 7, 6, 5]);
    assert_eq!(peer[0].packet.as_bytes()[28], 0);
    assert_eq!(peer[1].packet.as_bytes()[28], 2);
    let beacon = commands.iter().find(|command| command.kind == IbssCommandKind::Firmware && command.packet.as_bytes()[4] == 0x1c).unwrap();
    assert_eq!(&beacon.packet.as_bytes()[12..14], &[0x32, 0]);
    assert_eq!(commands.last().unwrap().kind, IbssCommandKind::Publish);
    assert_eq!(engine.bssid(), Some(MacAddress([2, 8, 7, 6, 5, 4])));
}

#[test]
fn failed_beacon_is_retained_and_tears_down_without_publication() {
    let mut engine = Ibss::new(); create(&mut engine);
    engine.command(1).unwrap().unwrap(); engine.complete(Ok(())).unwrap();
    assert_eq!(engine.command(2).unwrap().unwrap().packet.as_bytes()[4], 0x1c);
    assert_eq!(engine.complete(Err(AssociationIoError::Rejected)), Err(AssociationError::Physical(AssociationIoError::Rejected)));
    let cleanup = finish(&mut engine);
    assert_eq!(engine.failure(), Some(AssociationIoError::Rejected));
    assert_eq!(&cleanup[0].packet.as_bytes()[4..9], &[0x1c, 1, 0, 0, 2]);
    assert_eq!(cleanup.last().unwrap().kind, IbssCommandKind::Stopped);
    assert!(!cleanup.iter().any(|command| command.kind == IbssCommandKind::Publish));
    assert!(!engine.active());
}

#[test]
fn cancellation_removes_peer_and_rejects_late_completion() {
    let mut engine = Ibss::new();
    engine.set_peer(PeerIndex::new(3).unwrap(), PeerParameters::legacy()).unwrap();
    engine.merge(&candidate(2), LOCAL).unwrap(); finish(&mut engine);
    engine.cancel();
    assert_eq!(engine.complete(Ok(())), Err(AssociationError::StaleCompletion));
    let commands = finish(&mut engine);
    let codes: Vec<_> = commands.iter().filter(|command| command.kind == IbssCommandKind::Firmware)
        .map(|command| command.packet.as_bytes()[4]).collect();
    assert_eq!(codes, [0x1c, 0x18, 0x16]);
    assert_eq!(&commands[1].packet.as_bytes()[10..16], &[2, 9, 8, 7, 6, 5]);
    assert_eq!(commands.last().unwrap().kind, IbssCommandKind::Stopped);
}

#[test]
fn alone_timeout_requests_search_and_consumes_timer_once() {
    let mut engine = Ibss::new(); create(&mut engine); finish(&mut engine);
    engine.timer_expired().unwrap();
    assert_eq!(engine.timer_expired(), Err(AssociationError::StaleCompletion));
    let commands = finish(&mut engine);
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].kind, IbssCommandKind::Rescan);
    assert!(engine.active());
}

#[test]
fn teardown_and_recreation_preserve_generated_bssid() {
    let mut engine = Ibss::new(); create(&mut engine); finish(&mut engine);
    let bssid = engine.bssid();
    engine.cancel(); finish(&mut engine);
    engine.create(InterfaceMode::Auto, LOCAL, Ssid::parse(b"next").unwrap(),
        CreationParameters::new(Band::Ghz2, 11), Entropy([0xabcd; 3])).unwrap();
    finish(&mut engine);
    assert_eq!(engine.bssid(), bssid);
}

#[test]
fn peer_packet_carries_negotiated_ht_qos_and_typed_state() {
    let mut parameters = PeerParameters::legacy();
    parameters.kind = PeerKind::AccessPoint;
    parameters.association_id = 0x123;
    parameters.ht_capabilities = 0x62;
    parameters.receive_ampdu = 0x1b;
    parameters.ht_extended = 0x4567;
    parameters.beamforming = 0x89abcdef;
    parameters.antenna_selection = 0x25;
    parameters.mcs = 0x7f; parameters.mcs32 = true;
    parameters.qos = true; parameters.uapsd_supported = true;
    parameters.uapsd = 0xa5; parameters.service_period = 2;
    let packet = peer_packet(9, &candidate(2), PeerIndex::new(4).unwrap(), PeerState::Associated, &parameters);
    assert_eq!(&packet.as_bytes()[8..18], &[4, 0x41, 2, 9, 8, 7, 6, 5, 0x23, 1]);
    assert_eq!(&packet.as_bytes()[26..45], &[1, 1, 2, 0x7f, 1, 0x1b, 0x62, 0,
        0x67, 0x45, 0xef, 0xcd, 0xab, 0x89, 0x25, 0, 1, 0xa5, 2]);
}

#[test]
fn repeated_create_after_alone_search_does_not_reprogram_running_bss() {
    let mut engine = Ibss::new();
    engine.cancel();
    create(&mut engine); finish(&mut engine);
    engine.timer_expired().unwrap(); finish(&mut engine);
    let bssid = engine.bssid();
    create(&mut engine);
    assert!(engine.command(0).unwrap().is_none());
    assert_eq!(engine.bssid(), bssid);
}

#[test]
fn failed_disconnect_does_not_claim_the_target_bss_is_inactive() {
    let mut engine = Ibss::new(); create(&mut engine); finish(&mut engine);
    engine.cancel();
    loop {
        let command = engine.command(0).unwrap().unwrap();
        if command.kind == IbssCommandKind::Firmware && command.packet.as_bytes()[4] == 0x16 {
            assert_eq!(engine.complete(Err(AssociationIoError::Timeout)), Err(AssociationError::Physical(AssociationIoError::Timeout)));
            break;
        }
        engine.complete(Ok(())).unwrap();
    }
    finish(&mut engine);
    assert!(engine.active());
    assert_eq!(engine.failure(), Some(AssociationIoError::Timeout));
}

#[test]
fn infrastructure_bss_and_radio_updates_preserve_negotiated_operation() {
    let target = candidate(1);
    let mut peer = PeerParameters::legacy();
    peer.kind = PeerKind::AccessPoint; peer.qos = true;
    let radio = BssRadioParameters {
        secondary_channel: super::super::elements::SecondaryChannel::Above,
        erp_protection: true, ht_protection: HtProtection::NonHtMixed,
        non_greenfield_present: true, rifs_protection: true,
        basic_stbc: 0x1234, operation_protection: 0x4567, operation_information: 0x0d,
        short_preamble: false, short_slot: true,
    };
    let bss = infrastructure_bss_packet(5, &target, LOCAL, Authentication::Wpa2Psk,
        Encryption::Ccmp, PeerIndex::new(9).unwrap(), &peer, &radio);
    assert_eq!(&bss.as_bytes()[8..12], &[0, 0, 0, 6]);
    assert_eq!(bss.as_bytes()[50], 1);
    assert_eq!(bss.as_bytes()[56], 9);
    assert_eq!(&bss.as_bytes()[60..62], &[7, 6]);
    let expected_radio = [0, 1, 6, 1, 1, 3, 1, 1, 0x34, 0x12, 0x67, 0x45, 0x0d, 0, 1, 0x72];
    assert_eq!(&bss.as_bytes()[72..88], &expected_radio);
    let update = radio_packet(6, &target, &radio);
    assert_eq!(update.as_bytes()[4], 0x1d);
    assert_eq!(&update.as_bytes()[8..], &expected_radio);
}

#[test]
fn infrastructure_timing_and_peer_removal_have_distinct_target_layouts() {
    let target = candidate(1);
    let timing = timing_packet(7, &target, 0x123, 3, true);
    assert_eq!(&timing.as_bytes()[4..], &[0x1a, 1, 7, 0, 0, 3, 0x23, 1, 200, 0, 0, 0, 1, 0, 0, 0]);
    let removal = remove_peer_packet(8, PeerIndex::new(9).unwrap(), target.peer);
    assert_eq!(&removal.as_bytes()[4..], &[0x18, 1, 8, 0, 9, 0, 2, 9, 8, 7, 6, 5]);
}

#[test]
fn wmm_edca_programs_all_access_categories_and_station_qos() {
    use super::super::elements::AccessCategoryParameters;
    let categories = [
        AccessCategoryParameters { aifsn: 3, admission_required: false, contention_min: 15, contention_max: 1023, txop: 0 },
        AccessCategoryParameters { aifsn: 7, admission_required: false, contention_min: 15, contention_max: 1023, txop: 0 },
        AccessCategoryParameters { aifsn: 2, admission_required: true, contention_min: 7, contention_max: 15, txop: 94 },
        AccessCategoryParameters { aifsn: 2, admission_required: false, contention_min: 3, contention_max: 7, txop: 47 },
    ];
    let packet = edca_packet(11, &categories, true);
    assert_eq!(&packet.as_bytes()[..8], &[52, 0, 0, 0, 0x21, 1, 11, 0]);
    assert_eq!(&packet.as_bytes()[8..18], &[15, 0, 0xff, 3, 0, 0, 3, 0, 0, 0]);
    assert_eq!(&packet.as_bytes()[28..38], &[7, 0, 15, 0, 94, 0, 2, 0, 0, 1]);
    assert_eq!(&packet.as_bytes()[38..48], &[3, 0, 7, 0, 47, 0, 2, 0, 0, 0]);
    assert_eq!(&packet.as_bytes()[48..], &[0, 1, 0, 0]);
}

#[test]
fn canonical_pool_lease_selects_target_peer_and_network() {
    use super::super::super::super::vendor_wireless_peers::PeerPool;
    let mut pool = PeerPool::new();
    let _occupied = pool.reserve(NetIndex::PRIMARY, MacAddress([2, 1, 1, 1, 1, 1])).unwrap();
    let peer = MacAddress([2, 9, 8, 7, 6, 5]);
    let lease = pool.reserve(NetIndex::SECONDARY, peer).unwrap();
    let packet = peer_record_packet(3, NetIndex::SECONDARY, peer, &[0x82, 0x84],
        lease.index(), PeerState::Disconnected, &PeerParameters::legacy());
    assert_eq!(packet.as_bytes()[8], lease.index().wire());
    assert_eq!(packet.as_bytes()[20], NetIndex::SECONDARY.wire());
    assert_eq!(packet.as_bytes()[28], 0);
}

#[test]
fn independent_merge_requires_a_caller_owned_peer_index() {
    let mut engine = Ibss::new();
    assert_eq!(engine.merge(&candidate(2), LOCAL), Err(AssociationError::InvalidResponse));
    assert!(engine.command(0).unwrap().is_none());
}

#[test]
fn secondary_peer_body_preserves_role_and_has_no_transport_header() {
    let mut parameters = PeerParameters::legacy();
    parameters.kind = PeerKind::SecondaryClient;
    parameters.desired_phy = 0x12;
    parameters.basic_rates = 0x0540;
    let body = peer_record_body(NetIndex::SECONDARY, MacAddress([2, 1, 2, 3, 4, 5]),
        &[12, 24, 48], PeerIndex::new(11).unwrap(), PeerState::Disconnected, &parameters);
    assert_eq!(&body[..20], &[11, 0x22, 2, 1, 2, 3, 4, 5, 0, 0, 0, 0, 1, 0x12, 0x40, 5, 0x40, 5, 0, 0]);
    assert_eq!(&body[20..], &[0; 20]);
}

#[test]
fn pending_merge_keeps_its_peer_binding_between_device_completions() {
    let mut engine = Ibss::new();
    assert!(engine.ready_for_merge());
    engine.set_peer(PeerIndex::new(7).unwrap(), PeerParameters::legacy()).unwrap();
    engine.merge(&candidate(2), LOCAL).unwrap();
    assert!(!engine.ready_for_merge());
    let command = engine.command(0).unwrap().unwrap();
    assert_eq!(command.packet.as_bytes()[8], 7);
    engine.complete(Ok(())).unwrap();
    assert!(!engine.ready_for_merge());
    assert_eq!(engine.set_peer(PeerIndex::new(9).unwrap(), PeerParameters::legacy()), Err(AssociationError::Busy));
    let commands = finish(&mut engine);
    let associated = commands.iter().find(|command| command.kind == IbssCommandKind::Firmware
        && command.packet.as_bytes()[4] == 0x17).unwrap();
    assert_eq!(associated.packet.as_bytes()[8], 7);
    assert!(engine.ready_for_merge());
    engine.cancel();
    assert!(!engine.ready_for_merge());
    finish(&mut engine);
    assert!(engine.ready_for_merge());
}

#[test]
fn peer_command_keeps_basic_rates_separate_from_desired_operational_rates() {
    let mut parameters = PeerParameters::legacy();
    parameters.desired_rates = 0x000a;
    parameters.basic_rates = 0x0005;
    let packet = peer_packet(2, &candidate(2), PeerIndex::new(6).unwrap(), PeerState::Associated, &parameters);
    assert_eq!(&packet.as_bytes()[21..26], &[0x13, 0x0a, 0, 0x05, 0]);
}
