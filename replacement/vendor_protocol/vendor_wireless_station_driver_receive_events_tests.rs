use super::*;
use super::super::Packet;

fn wire(kind: u8, body: &[u8]) -> [u8; 112] {
    let mut packet = [0; 112];
    packet[..2].copy_from_slice(&((body.len() + 8) as u16).to_le_bytes());
    packet[2] = 1;
    packet[4] = kind;
    packet[8..8 + body.len()].copy_from_slice(body);
    packet
}
fn event(packet: &[u8]) -> FirmwareEvent<'_> {
    match Packet::parse(packet).unwrap() {
        Packet::Event(event) => event,
        _ => panic!("expected event packet"),
    }
}

#[test]
fn rssi_cache_expires_strictly_after_five_seconds_and_wraps() {
    let mut cpu = EventCpu::new();
    let first = wire(5, &[190, 160, 54, 0]);
    let next = wire(5, &[180, 150, 108, 0]);
    let start = u32::MAX - 2000;
    cpu.receive(&event(&first), EventTick(start)).unwrap();
    cpu.receive(&event(&next), EventTick(start.wrapping_add(5000))).unwrap();
    assert_eq!(cpu.signal().unwrap().signal, SignalDbm(-66));
    assert_eq!(cpu.speed(), Some(LinkSpeed(54)));
    cpu.receive(&event(&next), EventTick(start.wrapping_add(5001))).unwrap();
    assert_eq!(cpu.signal().unwrap().signal, SignalDbm(-76));
    assert_eq!(cpu.speed(), Some(LinkSpeed(108)));
}

#[test]
fn threshold_is_signed_inclusive_and_one_shot() {
    let mut cpu = EventCpu::new();
    cpu.arm_signal_trigger(SignalTrigger::Below(SignalDbm(-60)));
    let packet = wire(5, &[196, 160, 0, 0]);
    assert_eq!(cpu.receive(&event(&packet), EventTick(0)), Ok(EventOutcome::Signal {
        refreshed: true, threshold_crossed: Some(SignalDbm(-60)),
    }));
    assert_eq!(cpu.receive(&event(&packet), EventTick(1)), Ok(EventOutcome::Signal {
        refreshed: false, threshold_crossed: None,
    }));
    cpu.arm_signal_trigger(SignalTrigger::Above(SignalDbm(-65)));
    assert_eq!(cpu.receive(&event(&packet), EventTick(2)), Ok(EventOutcome::Signal {
        refreshed: false, threshold_crossed: Some(SignalDbm(-65)),
    }));
}

#[test]
fn both_statistics_events_publish_all_counters_and_timestamp() {
    let mut cpu = EventCpu::new();
    let first = wire(6, &[0xa5; 96]);
    cpu.receive(&event(&first), EventTick(10)).unwrap();
    assert_eq!(cpu.statistics(), Some(&Statistics { bytes: [0xa5; 96], tick: EventTick(10) }));
    let second = wire(15, &[0x5a; 96]);
    cpu.receive(&event(&second), EventTick(20)).unwrap();
    assert_eq!(cpu.statistics(), Some(&Statistics { bytes: [0x5a; 96], tick: EventTick(20) }));
    let short = wire(6, &[0xff; 95]);
    assert_eq!(cpu.receive(&event(&short), EventTick(30)), Err(EventError::Truncated));
    assert_eq!(cpu.statistics().unwrap().tick, EventTick(20));
}

#[test]
fn network_publication_preserves_body_not_overwritten_length() {
    let mut cpu = EventCpu::new();
    let packet = wire(12, &[0x11, 0x22, 0x33, 0x44, 0x55]);
    cpu.receive(&event(&packet), EventTick(0)).unwrap();
    assert_eq!(cpu.network_notification(), Some(&[2, 0, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x55][..]));
    let empty = wire(12, &[]);
    cpu.receive(&event(&empty), EventTick(0)).unwrap();
    assert_eq!(cpu.network_notification(), Some(&[2, 0, 0, 0][..]));
}

#[test]
fn reset_cancels_signal_trigger_and_invalidates_publications() {
    let mut cpu = EventCpu::new();
    cpu.arm_signal_trigger(SignalTrigger::Below(SignalDbm(0)));
    cpu.receive(&event(&wire(14, &[7])), EventTick(0)).unwrap();
    cpu.receive(&event(&wire(12, &[1])), EventTick(0)).unwrap();
    cpu.reset();
    assert_eq!(cpu.sleepy(), 0);
    assert_eq!(cpu.statistics(), None);
    assert_eq!(cpu.network_notification(), None);
    assert_eq!(cpu.receive(&event(&wire(5, &[200, 160, 0, 0])), EventTick(0)),
        Ok(EventOutcome::Signal { refreshed: true, threshold_crossed: None }));
}

#[test]
fn firmware_service_is_inert_but_nonlocal_cpu_is_not_success() {
    let mut cpu = EventCpu::new();
    assert_eq!(cpu.receive(&event(&wire(8, &[])), EventTick(0)), Ok(EventOutcome::Service));
    assert_eq!(cpu.receive(&event(&wire(17, &[0; 8])), EventTick(0)), Err(EventError::NotLocalEvent));
}

#[test]
fn power_save_quota_splits_follow_prior_queue_distribution() {
    let mut peer = HardwarePeerPower::new(true, true);
    assert_eq!(peer.receive(&event(&wire(26, &[3, 1, 0, 5]))), Ok(false));
    assert_eq!(peer.quotas(), (5, 3, 2));
    peer.receive(&event(&wire(30, &[3, 1, 1]))).unwrap();
    assert_eq!(peer.quotas(), (1, 1, 0));
    peer.receive(&event(&wire(30, &[3, 1, 6]))).unwrap();
    assert_eq!(peer.quotas(), (6, 4, 2));
    assert_eq!(peer.receive(&event(&wire(26, &[3, 0, 1, 0]))), Ok(true));
    assert!(!peer.asleep());
    assert_eq!(peer.receive(&event(&wire(30, &[3, 1, 99]))), Ok(false));
    assert_eq!(peer.quotas(), (6, 4, 2));
}

#[test]
fn non_uapsd_quota_uses_nondelivery_and_wraps_byte_arithmetic() {
    let mut peer = HardwarePeerPower::new(false, false);
    peer.receive(&event(&wire(26, &[0, 1, 0, 255]))).unwrap();
    assert_eq!(peer.quotas(), (255, 0, 255));
    peer.receive(&event(&wire(30, &[0, 2, 2]))).unwrap();
    assert_eq!(peer.quotas(), (1, 0, 1));
    peer.receive(&event(&wire(30, &[0, 3, 2]))).unwrap();
    assert_eq!(peer.quotas(), (255, 0, 255));
}

#[test]
fn invalid_quota_packet_does_not_change_peer_state() {
    let mut peer = HardwarePeerPower::new(true, true);
    assert_eq!(peer.receive(&event(&wire(26, &[0, 1, 4, 8]))), Err(EventError::InvalidQuotaOperation));
    assert!(!peer.asleep());
    assert_eq!(peer.quotas(), (0, 0, 0));
    assert_eq!(peer.receive(&event(&wire(26, &[0, 1, 0]))), Err(EventError::Truncated));
    assert!(!peer.asleep());
}

#[test]
fn bss_resume_wakes_transmit_only_on_absent_to_present_transition() {
    let mut bss = HardwareBssPower::new();
    assert_eq!(bss.receive(&event(&wire(25, &[1, 2, 10]))), Ok(false));
    assert!(bss.absent());
    assert_eq!(bss.free_quota(), 10);
    assert_eq!(bss.receive(&event(&wire(25, &[1, 0, 4]))), Ok(true));
    assert!(!bss.absent());
    assert_eq!(bss.free_quota(), 4);
    assert_eq!(bss.receive(&event(&wire(25, &[1, 0, 4]))), Ok(false));
}

#[test]
fn block_ack_events_control_actual_packet_publication() {
    use super::super::reorder::{PublicationKind, ReorderTick};
    use crate::vendor_wireless_network::MacAddress;
    use crate::vendor_wireless_peers::NetIndex;
    let mut peers = PeerPool::new();
    let peer = peers.reserve(NetIndex::PRIMARY, MacAddress([2, 0, 0, 0, 0, 1])).unwrap();
    let tid = Tid::new(3).unwrap();
    let mut reorder = ReorderEngine::<u32, 4>::new();
    let add = wire(17, &[0, 0, 0x0c, 0x02, 0, 0, 0x40, 0x06]);
    assert_eq!(receive_block_ack(&event(&add), &peers, &mut reorder), Ok(BlockAckDisposition::Added));
    reorder.push(peer, tid, SequenceNumber::new(100).unwrap(), 10, ReorderTick(0)).unwrap();
    let ready = reorder.pop_ready().unwrap();
    assert_eq!(ready.packet, 10);
    assert_eq!(ready.kind, PublicationKind::Deliver);
    reorder.push(peer, tid, SequenceNumber::new(102).unwrap(), 12, ReorderTick(0)).unwrap();
    assert!(reorder.pop_ready().is_none());
    let delete = wire(18, &[0, 3]);
    assert_eq!(receive_block_ack(&event(&delete), &peers, &mut reorder), Ok(BlockAckDisposition::Deleted));
    let ready = reorder.pop_ready().unwrap();
    assert_eq!(ready.packet, 12);
    assert_eq!(ready.kind, PublicationKind::Deliver);
    assert!(reorder.pop_ready().is_none());
}

#[test]
fn unknown_peer_block_ack_does_not_create_reorder_session() {
    let mut reorder = ReorderEngine::<u32, 1>::new();
    let mut peers = PeerPool::new();
    let packet = wire(17, &[19]);
    assert_eq!(receive_block_ack(&event(&packet), &peers, &mut reorder), Ok(BlockAckDisposition::UnknownPeer));
    assert!(reorder.pop_ready().is_none());
    peers.reserve(crate::vendor_wireless_peers::NetIndex::PRIMARY,
        crate::vendor_wireless_network::MacAddress([2, 0, 0, 0, 0, 1])).unwrap();
    let invalid_tid = wire(18, &[0, 8]);
    assert_eq!(receive_block_ack(&event(&invalid_tid), &peers, &mut reorder), Err(BlockAckError::InvalidTid));
}

#[test]
fn reused_peer_slot_cannot_delete_previous_generation_reorder() {
    use crate::vendor_wireless_network::MacAddress;
    use crate::vendor_wireless_peers::NetIndex;
    use super::super::reorder::{PublicationKind, ReorderTick};
    let mut peers = PeerPool::new();
    let old = peers.reserve(NetIndex::PRIMARY, MacAddress([2, 0, 0, 0, 0, 1])).unwrap();
    let mut reorder = ReorderEngine::<u32, 4>::new();
    let add = wire(17, &[0, 0, 0, 2, 0, 0, 0, 0]);
    receive_block_ack(&event(&add), &peers, &mut reorder).unwrap();
    let tid = Tid::new(0).unwrap();
    reorder.push(old, tid, SequenceNumber::new(0).unwrap(), 10, ReorderTick(0)).unwrap();
    assert_eq!(reorder.pop_ready().unwrap().packet, 10);
    reorder.push(old, tid, SequenceNumber::new(2).unwrap(), 12, ReorderTick(0)).unwrap();
    peers.begin_release(old).unwrap();
    peers.release(old).unwrap();
    let new = peers.reserve(NetIndex::PRIMARY, MacAddress([2, 0, 0, 0, 0, 2])).unwrap();
    assert_eq!(new.index(), old.index());
    assert_ne!(new, old);
    receive_block_ack(&event(&wire(18, &[0, 0])), &peers, &mut reorder).unwrap();
    assert!(reorder.pop_ready().is_none());
    reorder.teardown_peer(old, DeleteMode::Cancel);
    let publication = reorder.pop_ready().unwrap();
    assert_eq!(publication.packet, 12);
    assert_eq!(publication.kind, PublicationKind::Cancel);
    assert!(reorder.pop_ready().is_none());
}
