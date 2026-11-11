use super::*;
use runtime::SecondaryRuntime;
use wire::*;
use crate::vendor_wireless_adapter::FirmwareRequest;
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_peers::{CapabilityBits, HtCapabilities, NetIndex, PeerLease, PeerPool, PeerState, PhyFlags};
use crate::vendor_wireless_station_driver::association::ibss::{PeerKind, PeerParameters};
use crate::vendor_wireless_station_driver::receive::reorder::ReorderEngine;

fn projection() -> PeerProjection { PeerProjection { non_erp: false, protection: Protection::None, non_greenfield: false, secondary_offset: 5, short_preamble: true, short_slot: true } }
fn context(mode: BssMode) -> BssContext { BssContext { mode, active: true, short_preamble_allowed: true, secondary_offset: 1, coexistence_forced: false, connected: true, has_peer: false, beacon: BeaconStorage::Present } }
fn runtime(mode: BssMode, peer: Option<PeerLease>) -> SecondaryRuntime {
    SecondaryRuntime::new(context(mode), BssParameters { band: Band::Ghz2, channel: Channel::new(6).unwrap(), configured_secondary: 1, rifs: 0, ht_info3: 0x5678, ht_info2: 0x1234 }, Capability(0x421), InterfaceParameters { name: NetworkName::new(b"DIRECT-test").unwrap(), bssid: MacAddress([2, 3, 4, 5, 6, 7]), local: MacAddress([2, 9, 8, 7, 6, 5]), qos: 1, phy_index: 1, operational_rates: 0xfff, basic_rates: 0x15f, phy: 2, security_enabled: true, secondary_enabled: true }, projection(), peer)
}
fn capable() -> PeerCapabilities { PeerCapabilities { active: true, associated: true, secondary: true, phy: 0x12, ht: 0x12, capability: 0x420 } }
fn pool_with(capabilities: PeerCapabilities) -> (PeerPool, PeerLease) {
    let mut peers = PeerPool::new();
    let mac = MacAddress([2, 1, 3, 5, 7, 9]);
    let lease = peers.reserve(NetIndex::SECONDARY, mac).unwrap();
    let peer = peers.get_mut(lease).unwrap();
    peer.phy = PhyFlags(capabilities.phy);
    peer.ht = HtCapabilities(capabilities.ht);
    peer.capability = CapabilityBits(capabilities.capability);
    peers.set_state(lease, PeerState::Associated).unwrap();
    peers.complete_association(lease, mac).unwrap();
    (peers, lease)
}
fn parameters() -> PeerParameters { PeerParameters { kind: PeerKind::SecondaryClient, ..PeerParameters::legacy() } }

#[test]
fn schedule_preserves_all_three_word_fields_and_combines_flags() {
    let bytes = [1, 0, 0, 0x40, 0x34, 0x12, 7, 1, 2, 9, 0, 0, 0x78, 0x56, 0x34, 0x12, 0xef, 0xcd, 0xab, 0x90, 4, 3, 2, 1];
    let schedule = Schedule::parse(&bytes).unwrap();
    assert_eq!(schedule.interval, 0x1234);
    assert_eq!(schedule.index, 7);
    assert_eq!(schedule.combined_flags, ScheduleFlags(0x42));
    assert_eq!(schedule.entries(), &[ScheduleEntry { flags: ScheduleFlags(2), count: 9, duration: FirmwareDuration(0x12345678), interval: FirmwareDuration(0x90abcdef), start: FirmwareTime(0x01020304) }]);
}
#[test]
fn invalid_schedule_does_not_replace_published_schedule() {
    let mut cpu = SecondaryBss::new(context(BssMode::AccessPoint), projection());
    cpu.receive_schedule(&[1, 0, 0, 5, 0, 0, 3, 0]).unwrap();
    cpu.complete(Operation::PublishBeacon, Ok(())).unwrap();
    assert_eq!(cpu.receive_schedule(&[1, 0, 0, 0, 0, 0, 0, 9]), Err(Error::TooManySchedules));
    assert_eq!(cpu.schedule().unwrap().flags, ScheduleFlags(5));
    assert_eq!(cpu.receive_schedule(&[1, 0, 0, 0, 0, 0, 0, 1]), Err(Error::Truncated));
    assert_eq!(cpu.poll(), None);
}
#[test]
fn secondary_root_forces_beacon_even_when_peer_projection_is_unchanged() {
    let mut service = runtime(BssMode::AccessPoint, None);
    let (mut peers, _) = pool_with(capable());
    let mut reorder = ReorderEngine::<(), 8>::new();
    service.receive_root(&[1, 2, 3, 4, 5, 0], &peers).unwrap();
    service.prepare(BeaconElements::new(&[0, 1, b'x']).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.command(), Some(FirmwareRequest::BeaconTemplate(&[1, 1, 0, 0, 0x21, 4, 3, 0, 0, 1, b'x'])));
    assert_eq!(service.cpu.root().unwrap().second, 4);
}
#[test]
fn legacy_peer_changes_parameters_before_beacon() {
    let mut service = runtime(BssMode::AccessPoint, None);
    let legacy = PeerCapabilities { phy: 0, ht: 0, capability: 0, ..capable() };
    let (mut peers, _) = pool_with(legacy);
    let mut reorder = ReorderEngine::<(), 8>::new();
    service.receive_root(&[1, 0, 0, 0, 0, 0], &peers).unwrap();
    service.prepare(BeaconElements::new(&[]).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.command(), Some(FirmwareRequest::BssParameters(&[1, 1, 6, 1, 1, 3, 1, 0, 0x78, 0x56, 0x34, 0x12, 5, 0, 0, 0x72])));
    service.finish_command(Ok(()), &mut peers).unwrap();
    service.prepare(BeaconElements::new(&[]).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.command(), Some(FirmwareRequest::BeaconTemplate(&[1, 1, 0, 0, 0x21, 0, 0, 0])));
}
#[test]
fn inactive_foreign_and_unassociated_peers_do_not_change_projection() {
    let mut cpu = SecondaryBss::new(context(BssMode::AccessPoint), projection());
    let bad = PeerCapabilities { phy: 0, ht: 0, capability: 0, ..capable() };
    cpu.recompute(&[PeerCapabilities { active: false, ..bad }, PeerCapabilities { secondary: false, ..bad }, PeerCapabilities { associated: false, ..bad }], false).unwrap();
    assert_eq!(cpu.poll(), None);
}
#[test]
fn forty_mhz_intolerance_suppresses_secondary_channel() {
    let mut cpu = SecondaryBss::new(context(BssMode::AccessPoint), projection());
    cpu.receive_root(&[1, 0, 0, 0, 0, 0], &[PeerCapabilities { ht: 0x4012, ..capable() }]).unwrap();
    assert_eq!(cpu.projection().unwrap().secondary_offset, 0);
}
#[test]
fn secondary_root_blocking_byte_suppresses_secondary_channel() {
    let mut cpu = SecondaryBss::new(context(BssMode::AccessPoint), projection());
    cpu.receive_root(&[1, 0, 0, 0, 0, 7], &[capable()]).unwrap();
    assert_eq!(cpu.projection().unwrap().secondary_offset, 0);
}
#[test]
fn non_access_point_root_does_not_publish_or_change_root() {
    let mut cpu = SecondaryBss::new(context(BssMode::Client), projection());
    cpu.receive_root(&[1, 2, 3, 4, 5, 6], core::iter::empty::<PeerCapabilities>()).unwrap();
    assert_eq!(cpu.root(), None);
    assert_eq!(cpu.poll(), None);
}
#[test]
fn physical_failure_preserves_exact_pending_packet_until_explicit_retry() {
    let mut service = runtime(BssMode::AccessPoint, None);
    let mut peers = PeerPool::new();
    let mut reorder = ReorderEngine::<(), 8>::new();
    service.receive_schedule(&[1, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    service.prepare(BeaconElements::new(&[1, 0]).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.finish_command(Err(DeviceFailure(0xc0000001)), &mut peers), Err(Error::Device(DeviceFailure(0xc0000001))));
    assert_eq!(service.command(), None);
    assert_eq!(service.prepare(BeaconElements::new(&[]).unwrap(), &mut peers, &mut reorder, None, &[]), Err(Error::Busy));
    assert_eq!(service.retry(), Some(DeviceFailure(0xc0000001)));
    assert_eq!(service.command(), Some(FirmwareRequest::BeaconTemplate(&[1, 1, 0, 0, 0x21, 4, 2, 0, 1, 0])));
}
#[test]
fn abort_publishes_reason_then_removes_peer_disconnects_and_flushes() {
    let (mut peers, lease) = pool_with(capable());
    let mut service = runtime(BssMode::Client, Some(lease));
    let mut reorder = ReorderEngine::<(), 8>::new();
    service.set_coexistence_deadline(FirmwareTime(500));
    service.abort().unwrap();
    let beacon = BeaconElements::new(&[]).unwrap();
    let parameters = parameters();
    service.prepare(beacon, &mut peers, &mut reorder, Some(&parameters), &[0x82, 0x84]).unwrap();
    assert_eq!(service.take_notification(), Some(runtime::DisconnectPublication { reason: 4 }));
    match service.command().unwrap() { FirmwareRequest::PeerParameters(bytes) => { assert_eq!(bytes[0], lease.index().wire()); assert_eq!(&bytes[2..8], &[2, 1, 3, 5, 7, 9]); assert_eq!(bytes[20], 0); }, _ => panic!("peer command required") }
    assert_eq!(peers.get(lease).unwrap().state(), PeerState::Disconnected);
    service.finish_command(Ok(()), &mut peers).unwrap();
    service.prepare(beacon, &mut peers, &mut reorder, None, &[]).unwrap();
    match service.command().unwrap() { FirmwareRequest::InterfaceParameters(bytes) => { assert_eq!(&bytes[..4], &[1, 1, 0, 11]); assert_eq!(bytes[48], lease.index().wire()); }, _ => panic!("interface command required") }
    service.finish_command(Ok(()), &mut peers).unwrap();
    service.prepare(beacon, &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.command(), Some(FirmwareRequest::FlushInterface(&[1, 0, 0, 0])));
    service.finish_command(Ok(()), &mut peers).unwrap();
    assert_eq!(service.coexistence_deadline(), None);
    assert!(peers.get(lease).is_err());
    assert_eq!(service.peer(), None);
    assert!(!service.cpu.context.connected);
}
#[test]
fn authenticated_peer_uses_local_state_transition_without_peer_command() {
    let (mut peers, lease) = pool_with(capable());
    peers.set_state(lease, PeerState::Authenticated).unwrap();
    let mut service = runtime(BssMode::Client, Some(lease));
    let mut reorder = ReorderEngine::<(), 8>::new();
    service.abort().unwrap();
    service.prepare(BeaconElements::new(&[]).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert!(matches!(service.command(), Some(FirmwareRequest::InterfaceParameters(_))));
    assert_eq!(peers.get(lease).unwrap().state(), PeerState::Disconnected);
}
#[test]
fn abort_without_peer_only_publishes_disconnect() {
    let mut service = runtime(BssMode::Client, None);
    let mut peers = PeerPool::new();
    let mut reorder = ReorderEngine::<(), 8>::new();
    service.abort().unwrap();
    service.prepare(BeaconElements::new(&[]).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.take_notification(), Some(runtime::DisconnectPublication { reason: 4 }));
    assert_eq!(service.command(), None);
}
#[test]
fn beacon_projection_updates_erp_and_ht_operation() {
    let elements = [42, 1, 0xff, 61, 22, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let root = RootUpdate::parse(&[1, 1, 1, 0, 0, 0]).unwrap();
    let parameters = BssParameters { band: Band::Ghz2, channel: Channel::new(6).unwrap(), configured_secondary: 1, rifs: 0, ht_info3: 0x1234, ht_info2: 0 };
    let mut bytes = [0; 608];
    let length = BeaconElements::new(&elements).unwrap().encode_projected(Capability(0x421), parameters, projection(), Some(root), true, &mut bytes).unwrap();
    assert_eq!(&bytes[8..11], &[42, 1, 2]);
    assert_eq!(&bytes[11..19], &[61, 22, 6, 5, 1, 0, 0x34, 0x12]);
    assert_eq!(&bytes[length-16..length], &[74, 14, 20, 0, 10, 0, 44, 1, 200, 0, 20, 0, 5, 0, 25, 0]);
}
#[test]
fn beacon_limit_is_six_hundred_ie_bytes() {
    assert!(BeaconElements::new(&[0; 600]).is_ok());
    assert_eq!(BeaconElements::new(&[0; 601]), Err(Error::BeaconTooLarge));
    assert_eq!(RootUpdate::parse(&[0, 0, 0, 0, 0, 0]), Err(Error::WrongBss));
    assert_eq!(RootUpdate::parse(&[1, 0]), Err(Error::Truncated));
}

#[test]
fn parsed_receive_event_executes_secondary_schedule_cpu() {
    use crate::vendor_wireless_station_driver::receive::Packet;
    let wire = [16, 0, 1, 0, 0x1c, 3, 0, 0, 1, 0, 0, 0x40, 0x34, 0x12, 7, 0];
    let Packet::Event(event) = Packet::parse(&wire).unwrap() else { panic!("event required"); };
    let mut service = runtime(BssMode::AccessPoint, None);
    let mut peers = PeerPool::new();
    let mut reorder = ReorderEngine::<(), 8>::new();
    assert_eq!(service.receive(&event, &peers, runtime::AbortAdmission::Allowed), Ok(true));
    assert_eq!(service.cpu.schedule().unwrap().interval, 0x1234);
    assert_eq!(service.cpu.schedule().unwrap().combined_flags, ScheduleFlags(0x40));
    service.prepare(BeaconElements::new(&[0, 1, b'x']).unwrap(), &mut peers, &mut reorder, None, &[]).unwrap();
    assert_eq!(service.command(), Some(FirmwareRequest::BeaconTemplate(&[1, 1, 0, 0, 0x21, 4, 3, 0, 0, 1, b'x'])));
}

#[test]
fn abort_event_respects_adapter_admission() {
    use crate::vendor_wireless_station_driver::receive::Packet;
    let wire = [9, 0, 1, 0, 0x1b, 0, 0, 0, 1];
    let Packet::Event(event) = Packet::parse(&wire).unwrap() else { panic!("event required"); };
    let mut service = runtime(BssMode::Client, None);
    let peers = PeerPool::new();
    assert_eq!(service.receive(&event, &peers, runtime::AbortAdmission::Suppressed), Ok(false));
    assert_eq!(service.cpu.poll(), None);
    assert_eq!(service.receive(&event, &peers, runtime::AbortAdmission::Allowed), Ok(true));
    assert!(matches!(service.cpu.poll(), Some(Effect::DisconnectNotification { reason: 4 })));
}

#[test]
fn schedule_without_beacon_storage_keeps_schedule_without_device_command() {
    let mut configuration = context(BssMode::AccessPoint);
    configuration.beacon = BeaconStorage::Absent;
    let mut cpu = SecondaryBss::new(configuration, projection());
    cpu.receive_schedule(&[1, 0, 0, 7, 0, 0, 0, 0]).unwrap();
    assert_eq!(cpu.schedule().unwrap().flags, ScheduleFlags(7));
    assert_eq!(cpu.poll(), None);
}
