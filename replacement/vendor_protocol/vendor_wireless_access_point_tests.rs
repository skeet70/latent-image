use super::*;
use crate::vendor_wireless_access_point_authenticator::Milliseconds;
use crate::vendor_wireless_peers::PeerPool;

const LOCAL: MacAddress = MacAddress([2, 0, 0, 0, 0, 1]);
const PEER: MacAddress = MacAddress([2, 0, 0, 0, 0, 2]);
fn configuration() -> Configuration {
    Configuration { ssid: Ssid::parse(b"cam").unwrap(), channel: Channel::Number(ChannelNumber::new(6).unwrap()),
        security: Security::Open, key: NetworkKey::new(Security::Open, &[]).unwrap(), key_index: KeyIndex::new(0).unwrap(),
        country: Some(*b"US "), beacon_interval: Some(100), disable_11n: true }
}
fn engine() -> AccessPoint {
    let mut ap = AccessPoint::new(LOCAL);
    ap.supply_hardware(HardwareCapabilities::new(&[beacon::Rate(0x82), beacon::Rate(0x84), beacon::Rate(0x8b), beacon::Rate(0x96)], None, true, true).unwrap()).unwrap();
    ap
}
fn drain_stop_security(ap: &mut AccessPoint, pool: &mut PeerPool) {
    for _ in 0..128 {
        match ap.poll_teardown_security().unwrap() {
            Some(SecurityEffect::Registrar(_)) => ap.complete_registrar_io(true).unwrap(),
            Some(SecurityEffect::Authenticator(effect)) => ap.complete_authenticator(effect.id, true, pool).unwrap(),
            Some(SecurityEffect::WpsOutcome(_)) => {},
            None => return,
        }
    }
    panic!("security teardown did not finish");
}
fn run_to_started(ap: &mut AccessPoint, pool: &mut PeerPool) -> Vec<Command> {
    let mut commands = Vec::new();
    for _ in 0..128 {
        match ap.poll().expect("startup must produce an external effect") {
            Effect::Driver { operation, command } => { commands.push(command); ap.complete(operation, Ok(()), pool).unwrap(); }
            Effect::Started => return commands,
            effect => panic!("unexpected startup effect {effect:?}"),
        }
    }
    panic!("startup did not finish");
}
fn management(control: u16, body: &[u8]) -> Vec<u8> {
    let mut frame = vec![0; 24];
    frame[..2].copy_from_slice(&control.to_le_bytes()); frame[4..10].copy_from_slice(&LOCAL.0);
    frame[10..16].copy_from_slice(&PEER.0); frame[16..22].copy_from_slice(&LOCAL.0);
    frame.extend_from_slice(body); frame
}
fn authenticate(ap: &mut AccessPoint, pool: &mut PeerPool) {
    let reply = ap.receive_management(&management(0xb0, &[0,0,1,0,0,0]), 47, Milliseconds(10), pool).unwrap().unwrap();
    assert_eq!(&reply.packet.as_bytes()[24..], &[0,0,2,0,0,0]);
    assert!(ap.complete_management(reply.id, true, pool).unwrap().is_none());
}
fn associate(ap: &mut AccessPoint, pool: &mut PeerPool, acknowledged: bool) -> Option<PeerInstallation> {
    let reply = ap.receive_management(&management(0, &[0x21,4,10,0,0,3,b'c',b'a',b'm',1,4,0x82,0x84,0x8b,0x96]), 51, Milliseconds(20), pool).unwrap().unwrap();
    assert_eq!(&reply.packet.as_bytes()[26..28], &[0,0]);
    ap.complete_management(reply.id, acknowledged, pool).unwrap()
}
#[test]
fn request_decodes_wep_hex_and_automatic_channel() {
    let mut payload = [0; 136]; payload[..3].copy_from_slice(b"cam"); payload[32..36].copy_from_slice(&3u32.to_le_bytes());
    payload[44..48].copy_from_slice(&1u32.to_le_bytes()); payload[48..58].copy_from_slice(b"0011aAbBff");
    payload[112..116].copy_from_slice(&10u32.to_le_bytes()); payload[120..124].copy_from_slice(&19u32.to_le_bytes()); payload[132] = 3;
    let parsed = Configuration::parse(&payload).unwrap();
    assert_eq!(parsed.channel, Channel::Automatic); assert_eq!(parsed.channel.frequency_mhz(), 2412);
    assert_eq!(parsed.key.as_bytes(), &[0,0x11,0xaa,0xbb,0xff]); assert_eq!(parsed.key_index.get(), 3);
    assert_eq!(parsed.beacon_interval, None);
    payload[48] = b'g'; assert_eq!(Configuration::parse(&payload), Err(Error::Key));
}
#[test]
fn service_configuration_owns_resolved_pointer_targets_and_exact_copy44() {
    let mut raw = [0; 48]; raw[4..8].copy_from_slice(&3u32.to_le_bytes()); raw[8..12].copy_from_slice(&6u32.to_le_bytes());
    raw[12..16].copy_from_slice(&2u32.to_le_bytes()); raw[32..36].copy_from_slice(&100u32.to_le_bytes()); raw[44..].fill(0xff);
    let resolved = ServiceConfiguration::resolve(&raw, b"cam", &[], Some(b"US\0")).unwrap();
    assert_eq!(resolved.bytes(), &raw[..44]);
    assert_eq!(resolved.configuration().ssid.as_bytes(), b"cam");
    assert_eq!(resolved.configuration().country, Some([b'U',b'S',0]));
    assert!(resolved.configuration().disable_11n);
    assert!(matches!(ServiceConfiguration::resolve(&raw, b"other", &[], None), Err(Error::Truncated)));
}
#[test]
fn startup_waits_for_driver_commit_and_emits_real_beacon() {
    let mut ap = engine(); let mut pool = PeerPool::new(); ap.start(configuration()).unwrap();
    let commands = run_to_started(&mut ap, &mut pool);
    assert_eq!(ap.state(), State::Running);
    assert!(matches!(commands.first(), Some(Command::Associate { frequency_mhz: 2437, .. })));
    assert!(matches!(commands.last(), Some(Command::Commit)));
    let beacon = commands.iter().find_map(|command| if let Command::Beacon(beacon) = command { Some(beacon) } else { None }).unwrap();
    assert_eq!(&beacon.head.as_bytes()[..2], &[0x80,0]); assert_eq!(&beacon.head.as_bytes()[4..10], &[255;6]);
    assert_eq!(&beacon.head.as_bytes()[32..36], &[100,0,0x21,4]);
    assert_eq!(&beacon.head.as_bytes()[36..], &[0,3,b'c',b'a',b'm',1,4,0x82,0x84,0x8b,0x96,3,1,6]);
    assert!(ap.peers().next().is_none());
}
#[test]
fn failed_start_preserves_error_until_physical_teardown_and_can_retry() {
    let mut ap = engine(); let mut pool = PeerPool::new(); ap.start(configuration()).unwrap();
    let Effect::Driver { operation, .. } = ap.poll().unwrap() else { panic!() };
    ap.complete(operation, Err(DriverError(-17)), &mut pool).unwrap();
    assert_eq!(ap.state(), State::Stopping);
    drain_stop_security(&mut ap, &mut pool);
    let mut stopped = false;
    for _ in 0..32 {
        match ap.poll().unwrap() {
            Effect::Driver { operation, .. } => ap.complete(operation, Ok(()), &mut pool).unwrap(),
            Effect::Failed(Error::Driver(DriverError(-17))) => { stopped = true; break; },
            other => panic!("unexpected effect {other:?}"),
        }
    }
    assert!(stopped); assert_eq!(ap.state(), State::Failed);
    ap.start(configuration()).unwrap(); run_to_started(&mut ap, &mut pool);
    assert_eq!(ap.state(), State::Running);
}
#[test]
fn cancellation_waits_for_inflight_driver_and_cancels_registrar_timer() {
    let mut ap = engine(); let mut pool = PeerPool::new(); ap.start(configuration()).unwrap();
    let Effect::Driver { operation, .. } = ap.poll().unwrap() else { panic!() };
    ap.cancel().unwrap(); assert!(ap.poll().is_none()); ap.complete(operation, Ok(()), &mut pool).unwrap();
    drain_stop_security(&mut ap, &mut pool);
    loop { match ap.poll().unwrap() {
        Effect::Driver { operation, .. } => ap.complete(operation, Ok(()), &mut pool).unwrap(),
        Effect::Cancelled => break,
        other => panic!("unexpected effect {other:?}"),
    }}
    assert_eq!(ap.state(), State::Stopped); assert!(ap.configuration().is_none());
}
#[test]
fn peer_publication_requires_received_association_ack_and_real_station_install() {
    let mut ap = engine(); let mut pool = PeerPool::new(); ap.start(configuration()).unwrap(); run_to_started(&mut ap, &mut pool);
    authenticate(&mut ap, &mut pool); assert_eq!(ap.peer_signal(PEER), None);
    assert!(associate(&mut ap, &mut pool, false).is_none()); assert_eq!(pool.iter().count(), 0);
    let installation = associate(&mut ap, &mut pool, true).unwrap();
    assert_eq!(ap.peer_signal(PEER), None); assert!(!pool.get(installation.lease).unwrap().hardware_active());
    ap.complete_peer_install(installation.id, true, &mut pool).unwrap();
    assert_eq!(ap.peer_signal(PEER), Some(51)); assert!(pool.get(installation.lease).unwrap().security_authorized);
    assert_eq!(ap.peers().collect::<Vec<_>>(), vec![Peer { address: PEER, signal: 51 }]);
    let mut data = management(0x108, &[]); data[16..22].fill(0);
    ap.receive_peer_frame(&data, 63, &mut pool).unwrap(); assert_eq!(ap.peer_signal(PEER), Some(63));
    data[10..16].copy_from_slice(&[2,0,0,0,0,99]);
    assert_eq!(ap.receive_peer_frame(&data, 99, &mut pool), Err(Error::UnknownPeer));
    assert_eq!(ap.peer_signal(PEER), Some(63));
}
#[test]
fn disconnect_unpublishes_peer_but_keeps_lease_until_hardware_removal() {
    let mut ap = engine(); let mut pool = PeerPool::new(); ap.start(configuration()).unwrap(); run_to_started(&mut ap, &mut pool);
    authenticate(&mut ap, &mut pool); let installation = associate(&mut ap, &mut pool, true).unwrap();
    ap.complete_peer_install(installation.id, true, &mut pool).unwrap();
    ap.peer_disconnected(PEER, &mut pool).unwrap();
    assert_eq!(ap.peer_signal(PEER), None); assert!(pool.get(installation.lease).unwrap().retiring());
    ap.complete_peer_removal(installation.lease, Ok(()), &mut pool).unwrap();
    assert!(pool.get(installation.lease).is_err());
    assert_eq!(ap.complete_peer_removal(installation.lease, Ok(()), &mut pool), Err(Error::UnexpectedCompletion));
}
#[test]
fn unknown_peer_and_foreign_bssid_cannot_seed_enumeration() {
    let mut ap = engine(); let mut pool = PeerPool::new(); ap.start(configuration()).unwrap(); run_to_started(&mut ap, &mut pool);
    let frame = management(0, &[0x21,4,10,0,0,3,b'c',b'a',b'm',1,4,0x82,0x84,0x8b,0x96]);
    let reply = ap.receive_management(&frame, 250, Milliseconds(20), &mut pool).unwrap().unwrap();
    assert_ne!(&reply.packet.as_bytes()[26..28], &[0,0]);
    assert!(ap.complete_management(reply.id, true, &mut pool).unwrap().is_none());
    let mut foreign = management(0xb0, &[0,0,1,0,0,0]); foreign[16..22].copy_from_slice(&[2,0,0,0,0,8]);
    assert!(ap.receive_management(&foreign, 255, Milliseconds(20), &mut pool).unwrap().is_none());
    assert!(ap.peers().next().is_none()); assert_eq!(pool.iter().count(), 0);
}
