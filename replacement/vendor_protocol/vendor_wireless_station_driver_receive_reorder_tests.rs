use super::*;

#[derive(Debug, Eq, PartialEq)]
struct Lease(u16);
fn peer(value: u8) -> PeerLease {
    let mut pool = crate::vendor_wireless_peers::PeerPool::new();
    (0..=value).map(|index| pool.reserve(crate::vendor_wireless_peers::NetIndex::PRIMARY, crate::vendor_wireless_network::MacAddress([index; 6])).unwrap()).last().unwrap()
}
fn tid(value: u8) -> Tid { Tid::new(value).unwrap() }
fn seq(value: u16) -> SequenceNumber { SequenceNumber::new(value).unwrap() }
fn engine<const N: usize>(start: u16) -> ReorderEngine<Lease, N> {
    let mut engine = ReorderEngine::new();
    engine.add(peer(0), tid(0), seq(start), BlockAckWindow::new(0).unwrap()).unwrap();
    engine
}
fn receive<const N: usize>(engine: &mut ReorderEngine<Lease, N>, sequence: u16, tick: u32) {
    engine.push(peer(0), tid(0), seq(sequence), Lease(sequence), ReorderTick(tick)).unwrap();
}
fn output<const N: usize>(engine: &mut ReorderEngine<Lease, N>) -> Vec<(u16, PublicationKind)> {
    let mut values = Vec::new();
    while let Some(publication) = engine.pop_ready() { values.push((publication.packet.0, publication.kind)); }
    values
}
#[test]
fn holes_close_in_sequence_order_across_wrap() {
    let mut engine = engine::<8>(4094);
    receive(&mut engine, 4094, 10);
    receive(&mut engine, 0, 20);
    receive(&mut engine, 4095, 30);
    assert_eq!(output(&mut engine), vec![(4094, PublicationKind::Deliver), (4095, PublicationKind::Deliver), (0, PublicationKind::Deliver)]);
}
#[test]
fn duplicate_and_old_packets_are_not_delivered() {
    let mut engine = engine::<8>(10);
    receive(&mut engine, 10, 1);
    receive(&mut engine, 12, 2);
    receive(&mut engine, 12, 3);
    receive(&mut engine, 10, 4);
    receive(&mut engine, 11, 5);
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver), (12, PublicationKind::Discard), (10, PublicationKind::Discard), (11, PublicationKind::Deliver), (12, PublicationKind::Deliver)]);
}
#[test]
fn first_in_window_packet_rebases_negotiated_start() {
    let mut engine = engine::<4>(100);
    receive(&mut engine, 99, 10);
    receive(&mut engine, 102, 20);
    assert_eq!(output(&mut engine), vec![(99, PublicationKind::Deliver), (102, PublicationKind::Deliver)]);
}
#[test]
fn window_advance_publishes_evicted_packets_before_new_contiguous_packets() {
    let mut engine = engine::<8>(10);
    receive(&mut engine, 10, 1);
    receive(&mut engine, 12, 2);
    receive(&mut engine, 16, 3);
    receive(&mut engine, 14, 4);
    receive(&mut engine, 13, 5);
    receive(&mut engine, 15, 6);
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver), (12, PublicationKind::Deliver), (13, PublicationKind::Deliver), (14, PublicationKind::Deliver), (15, PublicationKind::Deliver), (16, PublicationKind::Deliver)]);
}
#[test]
fn timeout_skips_one_hole_and_rearms_next_hole_with_wrapping_ticks() {
    let mut engine = engine::<8>(10);
    receive(&mut engine, 10, u32::MAX - 20);
    receive(&mut engine, 12, u32::MAX - 10);
    receive(&mut engine, 14, u32::MAX - 5);
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver)]);
    engine.expire(ReorderTick(988));
    assert!(output(&mut engine).is_empty());
    engine.expire(ReorderTick(989));
    assert_eq!(output(&mut engine), vec![(12, PublicationKind::Deliver)]);
    engine.expire(ReorderTick(1988));
    assert!(output(&mut engine).is_empty());
    engine.expire(ReorderTick(1989));
    assert_eq!(output(&mut engine), vec![(14, PublicationKind::Deliver)]);
}
#[test]
fn block_ack_request_advances_past_missing_packets() {
    let mut engine = engine::<8>(10);
    receive(&mut engine, 10, 1);
    receive(&mut engine, 12, 2);
    engine.advance(peer(0), tid(0), seq(12), ReorderTick(3));
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver), (12, PublicationKind::Deliver)]);
}
#[test]
fn renegotiation_flushes_but_teardown_cancels_only_owned_buffered_leases() {
    let mut engine = engine::<8>(10);
    receive(&mut engine, 10, 1);
    receive(&mut engine, 12, 2);
    engine.add(peer(0), tid(0), seq(20), BlockAckWindow::new(0).unwrap()).unwrap();
    receive(&mut engine, 20, 3);
    receive(&mut engine, 22, 4);
    engine.teardown(DeleteMode::Cancel);
    engine.expire(ReorderTick(2000));
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver), (12, PublicationKind::Deliver), (20, PublicationKind::Deliver), (22, PublicationKind::Cancel)]);
    receive(&mut engine, 23, 2001);
    assert_eq!(output(&mut engine), vec![(23, PublicationKind::Discard)]);
}
#[test]
fn capacity_failure_returns_original_lease_and_draining_reuses_storage() {
    let mut engine = engine::<1>(10);
    receive(&mut engine, 10, 1);
    let error = engine.push(peer(0), tid(0), seq(11), Lease(77), ReorderTick(2)).unwrap_err();
    assert_eq!(error, (ReorderError::PacketsFull, Lease(77)));
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver)]);
    receive(&mut engine, 11, 3);
    assert_eq!(output(&mut engine), vec![(11, PublicationKind::Deliver)]);
}
#[test]
fn eight_negotiations_are_shared_but_peer_and_tid_sequences_are_independent() {
    let mut engine = ReorderEngine::<Lease, 8>::new();
    for value in 0..8 { engine.add(peer(value), tid(value), seq(100), BlockAckWindow::new(64).unwrap()).unwrap(); }
    assert_eq!(engine.add(peer(9), tid(0), seq(1), BlockAckWindow::new(0).unwrap()), Err(ReorderError::SessionsFull));
    engine.push(peer(0), tid(0), seq(100), Lease(1), ReorderTick(1)).unwrap();
    engine.push(peer(1), tid(1), seq(100), Lease(2), ReorderTick(1)).unwrap();
    engine.teardown_peer(peer(0), DeleteMode::Cancel);
    assert_eq!(output(&mut engine), vec![(1, PublicationKind::Deliver), (2, PublicationKind::Deliver)]);
    engine.add(peer(9), tid(0), seq(1), BlockAckWindow::new(0).unwrap()).unwrap();
}
#[test]
fn wire_boundaries_reject_invalid_queue_keys() {
    assert!(PeerIndex::new(20).is_none());
    assert!(Tid::new(8).is_none());
    assert!(SequenceNumber::new(4096).is_none());
    assert!(BlockAckWindow::new(1024).is_none());
    assert_eq!(BlockAckWindow::new(64).unwrap().packets(), 68);
    assert_eq!(ReorderDescriptor::parse(&[0; 8]), Err(DescriptorError::Truncated));
}

fn device_packet(control: u16, flags: u8, peer: u8) -> [u8; 20] {
    let mut wire = [0; 20];
    wire[..2].copy_from_slice(&16u16.to_le_bytes());
    wire[5] = flags;
    wire[6..8].copy_from_slice(&control.to_le_bytes());
    wire[8] = peer;
    wire
}
#[derive(Debug)]
struct BorrowedPacket<'a>(&'a [u8]);
#[test]
fn direct_device_frames_decode_tid_sequence_bar_and_keep_original_storage() {
    let first = device_packet(0x300a, 2, 2);
    let buffered = device_packet(0x300c, 2, 2);
    let bar = device_packet(0xb00c, 0, 2);
    let passthrough = device_packet(0, 0, 255);
    let invalid = device_packet(0, 2, 255);
    let mut engine = ReorderEngine::<BorrowedPacket<'_>, 8>::new();
    engine.add(peer(2), tid(3), seq(10), BlockAckWindow::new(0).unwrap()).unwrap();
    engine.push_descriptor(ReorderDescriptor::parse(&first).unwrap(), peer(2), BorrowedPacket(&first), ReorderTick(1)).unwrap();
    let publication = engine.pop_ready().unwrap();
    assert_eq!(publication.kind, PublicationKind::Deliver);
    assert!(core::ptr::eq(publication.packet.0.as_ptr(), first.as_ptr()));
    engine.push_descriptor(ReorderDescriptor::parse(&buffered).unwrap(), peer(2), BorrowedPacket(&buffered), ReorderTick(2)).unwrap();
    assert!(engine.pop_ready().is_none());
    engine.push_descriptor(ReorderDescriptor::parse(&bar).unwrap(), peer(2), BorrowedPacket(&bar), ReorderTick(3)).unwrap();
    let indicator = engine.pop_ready().unwrap();
    assert_eq!(indicator.kind, PublicationKind::Discard);
    assert!(core::ptr::eq(indicator.packet.0.as_ptr(), bar.as_ptr()));
    let released = engine.pop_ready().unwrap();
    assert_eq!(released.kind, PublicationKind::Deliver);
    assert!(core::ptr::eq(released.packet.0.as_ptr(), buffered.as_ptr()));
    engine.push_descriptor(ReorderDescriptor::parse(&passthrough).unwrap(), peer(2), BorrowedPacket(&passthrough), ReorderTick(4)).unwrap();
    assert_eq!(engine.pop_ready().unwrap().kind, PublicationKind::Deliver);
    engine.push_descriptor(ReorderDescriptor::parse(&invalid).unwrap(), peer(2), BorrowedPacket(&invalid), ReorderTick(5)).unwrap();
    assert_eq!(engine.pop_ready().unwrap().kind, PublicationKind::Discard);
}

#[test]
fn reincarnated_peer_cannot_publish_or_delete_previous_generation_packets() {
    use crate::vendor_wireless_peers::{NetIndex, PeerPool};
    use crate::vendor_wireless_network::MacAddress;
    let mut pool = PeerPool::new();
    let old = pool.reserve(NetIndex::PRIMARY, MacAddress([1; 6])).unwrap();
    let mut engine = ReorderEngine::<Lease, 8>::new();
    engine.add(old, tid(0), seq(10), BlockAckWindow::new(0).unwrap()).unwrap();
    engine.push(old, tid(0), seq(10), Lease(10), ReorderTick(1)).unwrap();
    engine.push(old, tid(0), seq(12), Lease(12), ReorderTick(2)).unwrap();
    assert_eq!(output(&mut engine), vec![(10, PublicationKind::Deliver)]);
    pool.begin_release(old).unwrap();
    engine.teardown_peer(old, DeleteMode::Cancel);
    pool.release(old).unwrap();
    let new = pool.reserve(NetIndex::PRIMARY, MacAddress([2; 6])).unwrap();
    assert_eq!(old.index(), new.index());
    assert_ne!(old.generation(), new.generation());
    engine.add(new, tid(0), seq(20), BlockAckWindow::new(0).unwrap()).unwrap();
    engine.delete(old, tid(0), DeleteMode::Publish);
    engine.push(old, tid(0), seq(20), Lease(120), ReorderTick(3)).unwrap();
    engine.push(new, tid(0), seq(20), Lease(20), ReorderTick(4)).unwrap();
    assert_eq!(output(&mut engine), vec![(12, PublicationKind::Cancel), (120, PublicationKind::Discard), (20, PublicationKind::Deliver)]);
}
