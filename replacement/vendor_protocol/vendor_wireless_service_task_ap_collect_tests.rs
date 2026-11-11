use super::*;
fn peer(index: u32) -> AcceptedClient { AcceptedClient { slot: SlotId::new(index).unwrap(), address: Ipv4Address([192, 168, 1, index as u8 + 1]), hardware: MacAddress([2, 0, 0, 0, 0, index as u8 + 1]) } }
#[test]
fn collection_expires_after_ten_seconds_from_start() {
    let mut collector = ServiceCollector::new(); collector.start(Milliseconds(50_000)).unwrap();
    assert!(collector.poll(Milliseconds(59_999)).is_none());
    assert_eq!(collector.poll(Milliseconds(60_000)).unwrap().entries().count(), 0);
    assert!(collector.poll(Milliseconds(60_001)).is_none());
}
#[test]
fn accepted_notification_uses_tcp_peer_identity_not_host_header() {
    let mut collector = ServiceCollector::new(); collector.start(Milliseconds(0)).unwrap(); collector.accept(peer(0)).unwrap();
    assert_eq!(collector.receive(peer(0).slot, b"NOTIFY * HTTP/1.1\r\nHOST:1.2.3.4\r\nIMPORTER: Desktop-1\r\n").unwrap(), NotificationResponse::Accepted);
    let batch = collector.poll(Milliseconds(10_000)).unwrap();
    let entry = batch.entries().next().unwrap();
    assert_eq!(entry.id, ServiceId(u32::from_le_bytes([192, 168, 1, 1])));
    assert_eq!(entry.address, peer(0).hardware);
    assert_eq!(entry.name, ServiceName::parse(b"Desktop-1").unwrap());
}
#[test]
fn rejects_invalid_name_and_returns_protocol_forbidden_response() {
    let mut collector = ServiceCollector::new(); collector.start(Milliseconds(0)).unwrap(); collector.accept(peer(0)).unwrap();
    let response = collector.receive(peer(0).slot, b"NOTIFY * HTTP/1.1\r\nIMPORTER: invalid name\r\n").unwrap();
    assert_eq!(response.bytes(), b"HTTP/1.1 403 Forbidden\r\n");
    assert_eq!(collector.poll(Milliseconds(10_000)).unwrap().entries().count(), 0);
}
#[test]
fn six_closed_clients_finish_collection_in_slot_order() {
    let mut collector = ServiceCollector::new(); collector.start(Milliseconds(0)).unwrap();
    for index in (0..6).rev() {
        let client = peer(index); collector.accept(client).unwrap();
        collector.receive(client.slot, b"NOTIFY * HTTP/1.1\r\nIMPORTER: Camera\r\n").unwrap(); collector.closed(client.slot).unwrap();
    }
    let batch = collector.poll(Milliseconds(1)).unwrap();
    let expected: Vec<_> = (0..6).map(|index| peer(index).hardware).collect();
    assert_eq!(batch.entries().map(|entry| entry.address).collect::<Vec<_>>(), expected);
}
#[test]
fn duplicate_close_does_not_complete_six_client_batch() {
    let mut collector = ServiceCollector::new(); collector.start(Milliseconds(0)).unwrap(); collector.accept(peer(0)).unwrap();
    for _ in 0..6 { collector.closed(peer(0).slot).unwrap(); }
    assert!(collector.poll(Milliseconds(1)).is_none());
}
#[test]
fn cancellation_discards_collection_without_publishing_batch() {
    let mut collector = ServiceCollector::new(); collector.start(Milliseconds(u32::MAX - 100)).unwrap(); collector.accept(peer(0)).unwrap();
    collector.receive(peer(0).slot, b"NOTIFY * HTTP/1.1\r\nIMPORTER: Camera\r\n").unwrap(); collector.cancel();
    assert!(collector.poll(Milliseconds(10_000)).is_none());
    collector.start(Milliseconds(20_000)).unwrap(); assert_eq!(collector.poll(Milliseconds(30_000)).unwrap().entries().count(), 0);
}
