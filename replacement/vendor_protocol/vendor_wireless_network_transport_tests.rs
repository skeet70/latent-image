use super::{FragmentReassembler, InternetChecksum, Ipv4Address, PacketError, ReassemblyOutcome,
    ReceiveOutcome, SocketHandle, SocketPhase, TcpEndpoint, TcpPort, TcpSequence, TransportError,
    TransportOutcome, TransportRuntime, TransportTick};
use super::super::packet::{HopLimit, PacketIdentification, UdpDatagram, UdpEndpoint};
use super::super::dhcp::UdpPort;

type Runtime = TransportRuntime<4, 128>;
fn endpoint(last: u8, port: u16) -> TcpEndpoint {
    TcpEndpoint { address: Ipv4Address([192, 168, 1, last]), port: TcpPort::parse(port).unwrap() }
}
fn length(outcome: TransportOutcome) -> usize {
    match outcome { TransportOutcome::Packet { length } => length, other => panic!("expected wire packet, got {other:?}") }
}
fn connected() -> (Runtime, SocketHandle, Runtime, SocketHandle) {
    let mut client = Runtime::new();
    let mut server = Runtime::new();
    let listener = server.listen(endpoint(2, 80), TcpSequence(900)).unwrap();
    let outgoing = client.connect(endpoint(1, 45000), endpoint(2, 80), TcpSequence(u32::MAX - 1)).unwrap();
    let mut first = [0; 256];
    let mut second = [0; 256];
    let count = length(client.poll(TransportTick(0), &mut first).unwrap());
    let count = length(server.receive_ipv4(&first[..count], &mut second).unwrap());
    let count = length(client.receive_ipv4(&second[..count], &mut first).unwrap());
    assert_eq!(server.receive_ipv4(&first[..count], &mut second), Ok(TransportOutcome::Buffered));
    let incoming = server.accept(listener).unwrap();
    assert_eq!(client.status(outgoing).unwrap().phase, SocketPhase::Established);
    (client, outgoing, server, incoming)
}

#[test]
fn handshake_stream_ack_and_eof_cross_sequence_wrap() {
    let (mut client, outgoing, mut server, incoming) = connected();
    let mut first = [0; 256];
    let mut second = [0; 256];
    assert_eq!(client.send(outgoing, b"camera"), Ok(6));
    let count = length(client.poll(TransportTick(1), &mut first).unwrap());
    let count = length(server.receive_ipv4(&first[..count], &mut second).unwrap());
    client.receive_ipv4(&second[..count], &mut first).unwrap();
    assert!(!client.status(outgoing).unwrap().write_pending);
    let mut read = [0; 3];
    assert_eq!(server.recv(incoming, &mut read), Ok(ReceiveOutcome::Bytes(3)));
    assert_eq!(&read, b"cam");
    assert_eq!(server.recv(incoming, &mut read), Ok(ReceiveOutcome::Bytes(3)));
    assert_eq!(&read, b"era");
    client.close(outgoing).unwrap();
    let count = length(client.poll(TransportTick(2), &mut first).unwrap());
    let count = length(server.receive_ipv4(&first[..count], &mut second).unwrap());
    client.receive_ipv4(&second[..count], &mut first).unwrap();
    assert_eq!(server.recv(incoming, &mut read), Ok(ReceiveOutcome::End));
    server.close(incoming).unwrap();
    let count = length(server.poll(TransportTick(3), &mut first).unwrap());
    let count = length(client.receive_ipv4(&first[..count], &mut second).unwrap());
    server.receive_ipv4(&second[..count], &mut first).unwrap();
    assert_eq!(server.status(incoming).unwrap().phase, SocketPhase::Closed);
    assert_eq!(client.status(outgoing).unwrap().phase, SocketPhase::TimeWait);
    client.poll(TransportTick(120_003), &mut first).unwrap();
    assert_eq!(client.status(outgoing).unwrap().phase, SocketPhase::Closed);
}

#[test]
fn retransmitted_data_is_delivered_once_and_cancel_resets_peer() {
    let (mut client, outgoing, mut server, incoming) = connected();
    let mut first = [0; 256];
    let mut second = [0; 256];
    client.send(outgoing, b"one").unwrap();
    let count = length(client.poll(TransportTick(1), &mut first).unwrap());
    server.receive_ipv4(&first[..count], &mut second).unwrap();
    let retry = length(client.poll(TransportTick(1001), &mut first).unwrap());
    server.receive_ipv4(&first[..retry], &mut second).unwrap();
    let mut read = [0; 32];
    assert_eq!(server.recv(incoming, &mut read), Ok(ReceiveOutcome::Bytes(3)));
    assert_eq!(&read[..3], b"one");
    assert_eq!(server.recv(incoming, &mut read), Err(TransportError::WouldBlock));
    client.cancel(outgoing).unwrap();
    let count = length(client.poll(TransportTick(1002), &mut first).unwrap());
    server.receive_ipv4(&first[..count], &mut second).unwrap();
    assert_eq!(server.recv(incoming, &mut read), Err(TransportError::Reset));
    assert_eq!(client.send(outgoing, b"late"), Err(TransportError::InvalidHandle));
}

#[test]
fn short_output_does_not_consume_send_and_checksum_failure_does_not_publish_bytes() {
    let (mut client, outgoing, mut server, incoming) = connected();
    client.send(outgoing, b"payload").unwrap();
    let mut short = [77; 40];
    assert_eq!(client.poll(TransportTick(1), &mut short), Err(TransportError::Capacity));
    assert_eq!(short, [77; 40]);
    let mut bytes = [0; 256];
    let mut reply = [0; 256];
    let count = length(client.poll(TransportTick(1), &mut bytes).unwrap());
    bytes[count - 1] ^= 1;
    assert_eq!(server.receive_ipv4(&bytes[..count], &mut reply), Err(TransportError::Packet(PacketError::Checksum)));
    assert_eq!(server.status(incoming).unwrap().readable, 0);
    bytes[count - 1] ^= 1;
    server.receive_ipv4(&bytes[..count], &mut reply).unwrap();
    assert_eq!(server.status(incoming).unwrap().readable, 7);
}

#[test]
fn failed_connect_expires_and_clear_invalidates_old_handles() {
    let mut runtime = Runtime::new();
    let handle = runtime.connect(endpoint(1, 1000), endpoint(2, 80), TcpSequence(10)).unwrap();
    let mut packet = [0; 256];
    for now in [0, 1000, 3000, 7000, 15000, 31000] {
        assert!(matches!(runtime.poll(TransportTick(now), &mut packet), Ok(TransportOutcome::Packet { .. })));
    }
    runtime.poll(TransportTick(63000), &mut packet).unwrap();
    assert_eq!(runtime.status(handle).unwrap().failure, Some(TransportError::TimedOut));
    runtime.clear();
    assert_eq!(runtime.status(handle), Err(TransportError::InvalidHandle));
    let replacement = runtime.connect(endpoint(1, 1000), endpoint(2, 80), TcpSequence(20)).unwrap();
    assert_ne!(replacement, handle);
    assert_eq!(runtime.cancel(handle), Err(TransportError::InvalidHandle));
}

#[test]
fn lost_window_update_recovers_and_stream_reads_across_buffer_end() {
    let (mut client, outgoing, mut server, incoming) = connected();
    let mut first = [0; 256];
    let mut second = [0; 256];
    assert_eq!(client.send(outgoing, &[1; 128]), Ok(128));
    let count = length(client.poll(TransportTick(1), &mut first).unwrap());
    let count = length(server.receive_ipv4(&first[..count], &mut second).unwrap());
    client.receive_ipv4(&second[..count], &mut first).unwrap();
    assert_eq!(client.send(outgoing, &[2; 64]), Err(TransportError::WouldBlock));
    let mut read = [0; 100];
    assert_eq!(server.recv(incoming, &mut read), Ok(ReceiveOutcome::Bytes(100)));
    assert_eq!(read, [1; 100]);
    server.poll(TransportTick(2), &mut second).unwrap();
    let count = length(client.poll(TransportTick(1001), &mut first).unwrap());
    let count = length(server.receive_ipv4(&first[..count], &mut second).unwrap());
    client.receive_ipv4(&second[..count], &mut first).unwrap();
    assert_eq!(client.send(outgoing, &[2; 64]), Ok(64));
    let count = length(client.poll(TransportTick(1002), &mut first).unwrap());
    server.receive_ipv4(&first[..count], &mut second).unwrap();
    let mut read = [0; 92];
    assert_eq!(server.recv(incoming, &mut read), Ok(ReceiveOutcome::Bytes(92)));
    assert_eq!(&read[..28], &[1; 28]);
    assert_eq!(&read[28..], &[2; 64]);
}

#[test]
fn expired_first_fragment_emits_reassembly_time_exceeded() {
    let (packet, total) = datagram();
    let mut runtime = FragmentReassembler::<1, 64>::new();
    let mut output = [0; 128];
    let mut piece = [0; 64];
    let count = fragment(&packet[..total], 0, 16, true, &mut piece);
    runtime.receive_ipv4(&piece[..count], TransportTick(0), &mut output).unwrap();
    assert_eq!(runtime.expire_ipv4(TransportTick(59_999), &mut output), Ok(TransportOutcome::Buffered));
    let count = length(runtime.expire_ipv4(TransportTick(60_000), &mut output).unwrap());
    assert_eq!(count, 56);
    assert_eq!(&output[12..16], &[192, 168, 1, 2]);
    assert_eq!(&output[16..20], &[192, 168, 1, 1]);
    assert_eq!(&output[20..22], &[11, 1]);
    assert_eq!(&output[28..56], &piece[..28]);
    let mut checksum = InternetChecksum::new();
    checksum.update(&output[..20]);
    assert_eq!(checksum.finish(), 0);
    let mut checksum = InternetChecksum::new();
    checksum.update(&output[20..count]);
    assert_eq!(checksum.finish(), 0);
    assert_eq!(runtime.expire_ipv4(TransportTick(60_001), &mut output), Ok(TransportOutcome::Buffered));
}

fn datagram() -> ([u8; 52], usize) {
    let mut bytes = [0; 52];
    let payload = b"abcdefghijklmnopqrstuvwx";
    let length = UdpDatagram { source: UdpEndpoint { address: endpoint(1, 1000).address, port: UdpPort(1000) },
        destination: UdpEndpoint { address: endpoint(2, 2000).address, port: UdpPort(2000) }, payload }
        .encode_ipv4(PacketIdentification(17), HopLimit(64), &mut bytes).unwrap();
    (bytes, length)
}
fn fragment(packet: &[u8], offset: usize, length: usize, more: bool, output: &mut [u8]) -> usize {
    output[..20].copy_from_slice(&packet[..20]);
    output[2..4].copy_from_slice(&((20 + length) as u16).to_be_bytes());
    let flags = (offset / 8) as u16 | if more { 0x2000 } else { 0 };
    output[6..8].copy_from_slice(&flags.to_be_bytes());
    output[10..12].fill(0);
    let mut checksum = InternetChecksum::new();
    checksum.update(&output[..20]);
    output[10..12].copy_from_slice(&checksum.finish().to_be_bytes());
    output[20..20 + length].copy_from_slice(&packet[20 + offset..20 + offset + length]);
    20 + length
}

#[test]
fn fragments_arrive_tail_first_and_decode_as_original_udp() {
    let (packet, total) = datagram();
    let mut runtime = FragmentReassembler::<2, 64>::new();
    let mut output = [0; 128];
    let mut piece = [0; 64];
    let count = fragment(&packet[..total], 16, 16, false, &mut piece);
    assert_eq!(runtime.receive_ipv4(&piece[..count], TransportTick(0), &mut output), Ok(ReassemblyOutcome::Buffered));
    let count = fragment(&packet[..total], 0, 16, true, &mut piece);
    assert_eq!(runtime.receive_ipv4(&piece[..count], TransportTick(1), &mut output), Ok(ReassemblyOutcome::Complete { length: total }));
    assert_eq!(UdpDatagram::parse_ipv4(&output[..total]).unwrap().payload, b"abcdefghijklmnopqrstuvwx");
}

#[test]
fn fragment_overlap_preserves_predecessor_and_timeout_does_not_mix_datagrams() {
    let (packet, total) = datagram();
    let mut runtime = FragmentReassembler::<1, 64>::new();
    let mut output = [0; 128];
    let mut first = [0; 64];
    let count = fragment(&packet[..total], 0, 24, true, &mut first);
    runtime.receive_ipv4(&first[..count], TransportTick(0), &mut output).unwrap();
    let mut second = [0; 64];
    let count = fragment(&packet[..total], 16, 16, false, &mut second);
    second[20..28].fill(0xff);
    assert_eq!(runtime.receive_ipv4(&second[..count], TransportTick(1), &mut output), Ok(ReassemblyOutcome::Complete { length: total }));
    assert_eq!(UdpDatagram::parse_ipv4(&output[..total]).unwrap().payload, b"abcdefghijklmnopqrstuvwx");
    let count = fragment(&packet[..total], 0, 16, true, &mut first);
    runtime.receive_ipv4(&first[..count], TransportTick(u32::MAX - 100), &mut output).unwrap();
    assert_eq!(runtime.expire(TransportTick(59_899)), 1);
    let count = fragment(&packet[..total], 16, 16, false, &mut second);
    assert_eq!(runtime.receive_ipv4(&second[..count], TransportTick(59_900), &mut output), Ok(ReassemblyOutcome::Buffered));
}

#[test]
fn simultaneous_active_open_completes_and_transfers_stream_data() {
    let mut first = Runtime::new();
    let mut second = Runtime::new();
    let first_handle = first.connect(endpoint(1, 8000), endpoint(2, 9000), TcpSequence(10)).unwrap();
    let second_handle = second.connect(endpoint(2, 9000), endpoint(1, 8000), TcpSequence(20)).unwrap();
    let mut first_syn = [0; 256];
    let mut second_syn = [0; 256];
    let mut first_reply = [0; 256];
    let mut second_reply = [0; 256];
    let first_length = length(first.poll(TransportTick(0), &mut first_syn).unwrap());
    let second_length = length(second.poll(TransportTick(0), &mut second_syn).unwrap());
    let first_length = length(first.receive_ipv4(&second_syn[..second_length], &mut first_reply).unwrap());
    let second_length = length(second.receive_ipv4(&first_syn[..first_length], &mut second_reply).unwrap());
    first.receive_ipv4(&second_reply[..second_length], &mut first_syn).unwrap();
    second.receive_ipv4(&first_reply[..first_length], &mut second_syn).unwrap();
    assert_eq!(first.status(first_handle).unwrap().phase, SocketPhase::Established);
    assert_eq!(second.status(second_handle).unwrap().phase, SocketPhase::Established);
    first.send(first_handle, b"open").unwrap();
    let count = length(first.poll(TransportTick(1), &mut first_syn).unwrap());
    second.receive_ipv4(&first_syn[..count], &mut second_syn).unwrap();
    let mut read = [0; 4];
    assert_eq!(second.recv(second_handle, &mut read), Ok(ReceiveOutcome::Bytes(4)));
    assert_eq!(&read, b"open");
}
