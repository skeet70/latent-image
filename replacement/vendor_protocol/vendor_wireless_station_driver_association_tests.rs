use super::*;

const LOCAL: MacAddress = MacAddress([2, 0, 0, 0, 0, 1]);
const PEER: MacAddress = MacAddress([2, 0, 0, 0, 0, 2]);

struct Fixture { engine: Association, peers: PeerPool }
impl Fixture {
    fn new() -> Self { Self { engine: Association::new(), peers: PeerPool::new() } }
    fn command(&mut self) -> Option<&AssociationCommand> {
        self.engine.drive(&mut self.peers).unwrap();
        self.engine.command()
    }
}
impl core::ops::Deref for Fixture {
    type Target = Association;
    fn deref(&self) -> &Association { &self.engine }
}
impl core::ops::DerefMut for Fixture {
    fn deref_mut(&mut self) -> &mut Association { &mut self.engine }
}

fn management(control: u16, source: MacAddress, destination: MacAddress, body: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 24];
    bytes[..2].copy_from_slice(&control.to_le_bytes());
    bytes[4..10].copy_from_slice(&destination.0);
    bytes[10..16].copy_from_slice(&source.0);
    bytes[16..22].copy_from_slice(&PEER.0);
    bytes.extend_from_slice(body);
    bytes
}
fn beacon() -> Vec<u8> {
    management(0x80, PEER, MacAddress([255; 6]),
        &[0,0,0,0,0,0,0,0,100,0,1,0,0,3,b'l',b'a',b'b',1,4,0x82,0x84,0x8b,0x96,3,1,6])
}
fn drain(engine: &mut Fixture) -> Vec<(CommandKind, Vec<u8>)> {
    let mut result = Vec::new();
    while let Some(command) = engine.command() {
        let token = command.token();
        result.push((command.kind(), command.bytes().to_vec()));
        engine.finish_command(token, Ok(())).unwrap();
    }
    result
}
fn joining() -> Fixture {
    let mut engine = Fixture::new();
    engine.set_local_address(LOCAL);
    engine.receive_management(&beacon(), SignalDbm(-30)).unwrap();
    engine.request(AssociationRequest::SelectSsid(Ssid::parse(b"lab").unwrap())).unwrap();
    drain(&mut engine);
    let token = engine.channel_token().unwrap();
    engine.channel_granted(token, Milliseconds(2000)).unwrap();
    engine
}

#[test]
fn matching_beacon_drives_real_channel_request_and_authentication_frame() {
    let mut engine = joining();
    let commands = drain(&mut engine);
    let frame = commands.iter().find(|(kind, _)| matches!(kind, CommandKind::Management { .. })).unwrap();
    assert_eq!(&frame.1[..2], &[0xb0, 0]);
    assert_eq!(&frame.1[4..10], &PEER.0);
    assert_eq!(&frame.1[10..16], &LOCAL.0);
    assert_eq!(&frame.1[24..30], &[0, 0, 1, 0, 0, 0]);
    assert_eq!(engine.publication(), None);
}

#[test]
fn authentication_response_builds_association_request_with_owned_security_elements() {
    let mut engine = joining();
    drain(&mut engine);
    engine.set_security_elements(&[48, 2, 1, 0]).unwrap();
    engine.receive_management(&management(0xb0, PEER, LOCAL, &[0,0,2,0,0,0]), SignalDbm(-30)).unwrap();
    let commands = drain(&mut engine);
    let frame = commands.iter().find(|(kind, _)| matches!(kind, CommandKind::Management { .. })).unwrap();
    assert_eq!(&frame.1[..2], &[0,0]);
    assert!(frame.1.windows(5).any(|bytes| bytes == [0,3,b'l',b'a',b'b']));
    assert!(frame.1.windows(4).any(|bytes| bytes == [48,2,1,0]));
    assert_eq!(engine.publication(), None);
}

#[test]
fn association_success_publishes_only_after_prior_effects_complete() {
    let mut engine = joining();
    drain(&mut engine);
    engine.receive_management(&management(0xb0, PEER, LOCAL, &[0,0,2,0,0,0]), SignalDbm(-30)).unwrap();
    drain(&mut engine);
    engine.receive_management(&management(0x10, PEER, LOCAL, &[1,0,0,0,0x23,0xc1]), SignalDbm(-30)).unwrap();
    assert_eq!(engine.publication(), None);
    let effects = drain(&mut engine);
    assert!(effects.iter().any(|(kind, bytes)| *kind == CommandKind::Firmware && bytes[4] == 0x20 && bytes[10] == 1));
    assert_eq!(engine.publication(), Some(LinkPublication::Associated { peer: PEER, bssid: PEER, association_id: 0x123 }));
}

#[test]
fn unrelated_peer_cannot_authenticate_or_disconnect_selected_link() {
    let mut engine = joining();
    drain(&mut engine);
    let stranger = MacAddress([2,0,0,0,0,9]);
    engine.receive_management(&management(0xb0, stranger, LOCAL, &[0,0,2,0,0,0]), SignalDbm(-1)).unwrap();
    assert!(engine.command().is_none());
    assert_eq!(engine.publication(), None);
    assert_eq!(engine.state(), AssociationState::Join);
}

#[test]
fn scan_encodes_device_command_instead_of_cpu_message_selector() {
    let mut engine = Fixture::new();
    engine.request(AssociationRequest::SelectSsid(Ssid::parse(b"lab").unwrap())).unwrap();
    let command = engine.command().unwrap();
    assert_eq!(command.kind(), CommandKind::Firmware);
    assert_eq!(command.bytes()[4], 0x1e);
    assert_eq!(&command.bytes()[16..19], b"lab");
    assert_eq!(command.bytes().len(), 0x76);
    assert_eq!(u16::from_le_bytes([command.bytes()[0],command.bytes()[1]]) as usize, command.bytes().len());
}

#[test]
fn four_ssid_scan_preserves_each_length_and_channel_plan() {
    let mut engine = Fixture::new();
    let ssids = [Ssid::parse(b"one").unwrap(),Ssid::parse(b"two").unwrap(),Ssid::parse(b"three").unwrap(),Ssid::parse(b"four").unwrap()];
    let channels = [ScanChannel::parse(ScanBand::Ghz2,6).unwrap(),ScanChannel::parse(ScanBand::Ghz5,36).unwrap()];
    engine.configure_scan(ScanPlan::new(&ssids,&channels,ScanType::Active,ScanBand::Specified,Milliseconds(110),Milliseconds(7)).unwrap()).unwrap();
    engine.scan().unwrap();
    let bytes = engine.command().unwrap().bytes();
    assert_eq!(bytes[4],4);
    assert_eq!(&bytes[16..19],b"one");
    assert_eq!(&bytes[52..55],b"two");
    assert_eq!(&bytes[88..93],b"three");
    assert_eq!(&bytes[124..128],b"four");
    assert_eq!(&bytes[160..166],&[4,2,1,6,2,36]);
    assert_eq!(&bytes[156..158],&[7,0]);
}

#[test]
fn stale_physical_and_timer_completions_do_not_advance_new_work() {
    let mut engine = joining();
    let stale = engine.command().unwrap().token();
    let effects = drain(&mut engine);
    let timer = effects.iter().find_map(|(kind,_)| match kind { CommandKind::ArmTimer {token,delay} if *delay == Milliseconds(524) => Some(*token), _ => None }).unwrap();
    assert_eq!(engine.finish_command(stale,Ok(())),Err(AssociationError::StaleCompletion));
    engine.request(AssociationRequest::StopStation).unwrap();
    drain(&mut engine);
    assert_eq!(engine.expire(timer),Err(AssociationError::StaleCompletion));
    assert_eq!(engine.state(),AssociationState::Idle);
}

#[test]
fn transmission_failure_retries_then_releases_channel_without_success_publication() {
    let mut engine = joining();
    while engine.command().is_some_and(|command| !matches!(command.kind(),CommandKind::Management {..})) {
        let token=engine.command().unwrap().token(); engine.finish_command(token,Ok(())).unwrap();
    }
    let token=engine.command().unwrap().token();
    engine.finish_command(token,Err(AssociationIoError::Rejected)).unwrap();
    let effects=drain(&mut engine);
    let timer=effects.iter().find_map(|(kind,_)| match kind {CommandKind::ArmTimer{token,delay} if *delay==Milliseconds(102)=>Some(*token),_=>None}).unwrap();
    engine.expire(timer).unwrap();
    let token=engine.command().unwrap().token();
    engine.finish_command(token,Err(AssociationIoError::Rejected)).unwrap();
    let effects=drain(&mut engine);
    let timer=effects.iter().find_map(|(kind,_)| match kind {CommandKind::ArmTimer{token,delay} if *delay==Milliseconds(102)=>Some(*token),_=>None}).unwrap();
    engine.expire(timer).unwrap();
    drain(&mut engine);
    assert_eq!(engine.state(),AssociationState::WaitForNextScan);
    assert_eq!(engine.publication(),None);
}

#[test]
fn truncated_beacon_and_security_elements_leave_no_effect() {
    let mut engine=Fixture::new();
    assert_eq!(engine.receive_management(&[0;23],SignalDbm(-30)),Err(AssociationError::TruncatedFrame));
    assert_eq!(engine.set_security_elements(&[48,4,1]),Err(AssociationError::TruncatedElements));
    assert!(engine.command().is_none());
}

#[test]
fn cancellation_retires_inflight_scan_and_rejects_late_source_events() {
    let mut engine = Fixture::new();
    engine.begin_scan(None).unwrap();
    let old_command = engine.command().unwrap().token();
    let old_scan = engine.scan_token().unwrap();
    engine.cancel();
    assert_eq!(engine.finish_command(old_command, Ok(())), Err(AssociationError::StaleCompletion));
    assert_eq!(engine.scan_complete(old_scan), Err(AssociationError::StaleCompletion));
    let cleanup = drain(&mut engine);
    assert!(cleanup.iter().any(|(kind, bytes)| *kind == CommandKind::Firmware && bytes[4] == 0x1f));
    assert_eq!(engine.state(), AssociationState::Idle);
    assert!(engine.command().is_none());
}

#[test]
fn independent_creation_requires_real_entropy_and_emits_bss_beacon_and_timing_packets() {
    let mut engine = Fixture::new();
    engine.set_local_address(LOCAL);
    engine.request(AssociationRequest::SetInterfaceMode(InterfaceMode::Adhoc)).unwrap();
    drain(&mut engine);
    engine.request(AssociationRequest::SelectSsid(Ssid::parse(b"mesh").unwrap())).unwrap();
    drain(&mut engine);
    let scan = engine.scan_token().unwrap();
    engine.scan_complete(scan).unwrap();
    let command = engine.command().unwrap();
    assert_eq!(command.kind(), CommandKind::Entropy);
    let token = command.token();
    engine.supply_entropy(token, ibss::Entropy([0x0101, 0x0202, 0x0303])).unwrap();
    let effects = drain(&mut engine);
    assert!(effects.iter().any(|(kind,bytes)| *kind == CommandKind::Firmware && bytes[4] == 0x16));
    assert!(effects.iter().any(|(kind,bytes)| *kind == CommandKind::Firmware && bytes[4] == 0x1c));
    assert!(effects.iter().any(|(kind,bytes)| *kind == CommandKind::Firmware && bytes[4] == 0x19));
    assert_eq!(engine.publication(), None);
}

#[test]
fn existing_peer_ownership_selects_nonzero_management_station_and_retains_other_owner() {
    let mut engine = Fixture::new();
    let stranger = MacAddress([2,0,0,0,0,9]);
    let existing = engine.peers.reserve(NetIndex::PRIMARY, stranger).unwrap();
    engine.set_local_address(LOCAL);
    engine.receive_management(&beacon(), SignalDbm(-30)).unwrap();
    engine.request(AssociationRequest::SelectSsid(Ssid::parse(b"lab").unwrap())).unwrap();
    drain(&mut engine);
    let channel = engine.channel_token().unwrap();
    engine.channel_granted(channel, Milliseconds(2000)).unwrap();
    loop {
        let command = engine.command().unwrap();
        if matches!(command.kind(), CommandKind::Management { .. }) {
            let metadata = command.metadata().unwrap();
            assert_eq!(metadata.station, StationIndex(1));
            assert_eq!(metadata.packet_kind, PacketKind::Three);
            break;
        }
        let token = command.token();
        engine.finish_command(token, Ok(())).unwrap();
    }
    engine.cancel();
    drain(&mut engine);
    assert_eq!(engine.peers.lookup(NetIndex::PRIMARY, PEER), None);
    assert_eq!(engine.peers.lookup(NetIndex::PRIMARY, stranger), Some(existing));
}

#[test]
fn failed_scan_reports_its_source_identity_without_completing_successfully() {
    let mut engine = Fixture::new();
    engine.begin_scan(None).unwrap();
    let scan = engine.scan_token().unwrap();
    let command = engine.command().unwrap().token();
    engine.finish_command(command, Err(AssociationIoError::Removed)).unwrap();
    assert_eq!(engine.scan_failure(), Some((scan, AssociationError::Physical(AssociationIoError::Removed))));
    assert_eq!(engine.scan_completion(), None);
    drain(&mut engine);
    engine.begin_scan(None).unwrap();
    assert_eq!(engine.scan_failure(), None);
}
