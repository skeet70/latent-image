use crate::vendor_wireless_peers::{PeerIndex, PeerLease};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tid(u8);
impl Tid {
    pub const fn new(value: u8) -> Option<Self> { if value < 8 { Some(Self(value)) } else { None } }
    pub const fn get(self) -> u8 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SequenceNumber(u16);
impl SequenceNumber {
    pub const fn new(value: u16) -> Option<Self> { if value < 4096 { Some(Self(value)) } else { None } }
    pub const fn get(self) -> u16 { self.0 }
    fn advance(self, count: u16) -> Self { Self(self.0.wrapping_add(count) & 4095) }
    fn distance(self, other: Self) -> u16 { other.0.wrapping_sub(self.0) & 4095 }
    fn before(self, other: Self) -> bool { let distance = self.distance(other); distance != 0 && distance < 2048 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockAckWindow(u16);
impl BlockAckWindow {
    pub const fn new(negotiated: u16) -> Option<Self> { if negotiated <= 1023 { Some(Self(negotiated + 4)) } else { None } }
    pub const fn packets(self) -> u16 { self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReorderTick(pub u32);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeleteMode { Publish, Cancel }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReorderError { SessionsFull, PacketsFull }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublicationKind { Deliver, Discard, Cancel }
pub struct Publication<P> { pub packet: P, pub kind: PublicationKind }

struct Node<P> { packet: P, sequence: SequenceNumber, next: Option<usize>, kind: PublicationKind }
#[derive(Clone, Copy)]
struct Session {
    peer: PeerLease, tid: Tid, start: SequenceNumber, window: BlockAckWindow,
    first: bool, head: Option<usize>, hole_since: Option<ReorderTick>,
}

pub struct ReorderEngine<P, const N: usize> {
    sessions: [Option<Session>; 8],
    packets: [Option<Node<P>>; N],
    free: [usize; N],
    free_count: usize,
    ready_head: Option<usize>,
    ready_tail: Option<usize>,
}
impl<P, const N: usize> Default for ReorderEngine<P, N> { fn default() -> Self { Self::new() } }
impl<P, const N: usize> ReorderEngine<P, N> {
    pub fn new() -> Self {
        Self { sessions: [None; 8], packets: core::array::from_fn(|_| None), free: core::array::from_fn(|index| index), free_count: N, ready_head: None, ready_tail: None }
    }
    fn session(&self, peer: PeerLease, tid: Tid) -> Option<usize> {
        self.sessions.iter().position(|entry| entry.as_ref().is_some_and(|s| s.peer == peer && s.tid == tid))
    }
    pub fn add(&mut self, peer: PeerLease, tid: Tid, start: SequenceNumber, window: BlockAckWindow) -> Result<(), ReorderError> {
        self.delete(peer, tid, DeleteMode::Publish);
        let index = self.sessions.iter().position(Option::is_none).ok_or(ReorderError::SessionsFull)?;
        self.sessions[index] = Some(Session { peer, tid, start, window, first: true, head: None, hole_since: None });
        Ok(())
    }
    fn publish(&mut self, index: usize, kind: PublicationKind) {
        let node = self.packets[index].as_mut().unwrap();
        node.next = None;
        node.kind = kind;
        if let Some(tail) = self.ready_tail { self.packets[tail].as_mut().unwrap().next = Some(index); }
        else { self.ready_head = Some(index); }
        self.ready_tail = Some(index);
    }
    pub fn pop_ready(&mut self) -> Option<Publication<P>> {
        let index = self.ready_head?;
        let node = self.packets[index].take().unwrap();
        self.ready_head = node.next;
        if self.ready_head.is_none() { self.ready_tail = None; }
        self.free[self.free_count] = index;
        self.free_count += 1;
        Some(Publication { packet: node.packet, kind: node.kind })
    }
    pub fn delete(&mut self, peer: PeerLease, tid: Tid, mode: DeleteMode) {
        if let Some(index) = self.session(peer, tid) {
            let mut session = self.sessions[index].take().unwrap();
            while let Some(head) = session.head {
                session.head = self.packets[head].as_ref().unwrap().next;
                self.publish(head, match mode { DeleteMode::Publish => PublicationKind::Deliver, DeleteMode::Cancel => PublicationKind::Cancel });
            }
        }
    }
    pub fn teardown_peer(&mut self, peer: PeerLease, mode: DeleteMode) {
        for tid in 0..8 { self.delete(peer, Tid(tid), mode); }
    }
    pub fn teardown(&mut self, mode: DeleteMode) {
        for slot in 0..8 {
            if let Some(session) = self.sessions[slot] { self.delete(session.peer, session.tid, mode); }
        }
    }
    fn release(&mut self, session: &mut Session, now: ReorderTick, advance: bool, timeout: bool) {
        let mut expired = timeout && session.hole_since.is_some_and(|tick| (now.0.wrapping_sub(tick.0) as i32) >= 1000);
        while let Some(head) = session.head {
            let node = self.packets[head].as_ref().unwrap();
            let sequence = node.sequence;
            if sequence == session.start { session.start = sequence.advance(1); }
            else if advance && sequence.before(session.start) {}
            else if expired { session.start = sequence.advance(1); expired = false; session.hole_since = None; }
            else { break; }
            session.head = node.next;
            self.publish(head, PublicationKind::Deliver);
        }
        if session.head.is_none() { session.hole_since = None; }
        else if session.hole_since.is_none() { session.hole_since = Some(now); }
    }
    fn retain(&mut self, packet: P, sequence: SequenceNumber) -> Result<usize, (ReorderError, P)> {
        if self.free_count == 0 { return Err((ReorderError::PacketsFull, packet)); }
        self.free_count -= 1;
        let index = self.free[self.free_count];
        self.packets[index] = Some(Node { packet, sequence, next: None, kind: PublicationKind::Deliver });
        Ok(index)
    }
    pub fn push(&mut self, peer: PeerLease, tid: Tid, sequence: SequenceNumber, packet: P, now: ReorderTick) -> Result<(), (ReorderError, P)> {
        let index = self.retain(packet, sequence)?;
        let Some(slot) = self.session(peer, tid) else { self.publish(index, PublicationKind::Discard); return Ok(()); };
        let mut session = self.sessions[slot].take().unwrap();
        let distance = session.start.distance(sequence);
        if distance >= 2048 {
            self.publish(index, if session.first { PublicationKind::Deliver } else { PublicationKind::Discard });
        } else {
            let ahead = distance >= session.window.0;
            if session.first {
                session.first = false;
                session.start = if ahead { sequence.advance(4096 - session.window.0 + 1) } else { sequence };
            } else if ahead { session.start = sequence.advance(4096 - session.window.0 + 1); }
            let mut previous = None;
            let mut current = session.head;
            let mut duplicate = false;
            while let Some(cursor) = current {
                let node = self.packets[cursor].as_ref().unwrap();
                if node.sequence == sequence { duplicate = true; break; }
                if sequence.before(node.sequence) { break; }
                previous = current;
                current = node.next;
            }
            if duplicate { self.publish(index, PublicationKind::Discard); }
            else {
                self.packets[index].as_mut().unwrap().next = current;
                if let Some(previous) = previous { self.packets[previous].as_mut().unwrap().next = Some(index); }
                else { session.head = Some(index); }
            }
            self.release(&mut session, now, ahead, !ahead);
        }
        self.sessions[slot] = Some(session);
        Ok(())
    }
    pub fn advance(&mut self, peer: PeerLease, tid: Tid, sequence: SequenceNumber, now: ReorderTick) {
        if let Some(slot) = self.session(peer, tid) {
            let mut session = self.sessions[slot].take().unwrap();
            if session.start.before(sequence) { session.start = sequence; self.release(&mut session, now, true, false); }
            self.sessions[slot] = Some(session);
        }
    }
    pub fn expire(&mut self, now: ReorderTick) {
        for slot in 0..8 {
            if let Some(mut session) = self.sessions[slot].take() {
                self.release(&mut session, now, false, true);
                self.sessions[slot] = Some(session);
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError { Truncated }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ReorderRoute { Immediate, Reorder, Advance }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReorderDescriptor { peer: Option<PeerIndex>, tid: Tid, sequence: SequenceNumber, route: ReorderRoute }
impl ReorderDescriptor {
    pub fn parse(wire: &[u8]) -> Result<Self, DescriptorError> {
        let wire = wire.get(..9).ok_or(DescriptorError::Truncated)?;
        let control = u16::from_le_bytes([wire[6], wire[7]]);
        let route = if control & 0x8000 != 0 { ReorderRoute::Advance }
            else if wire[5] & 2 != 0 { ReorderRoute::Reorder } else { ReorderRoute::Immediate };
        Ok(Self { peer: PeerIndex::new(wire[8]), tid: Tid(((control >> 12) & 7) as u8), sequence: SequenceNumber(control & 4095), route })
    }
    pub const fn peer(self) -> Option<PeerIndex> { self.peer }
}
impl<P, const N: usize> ReorderEngine<P, N> {
    pub fn push_descriptor(&mut self, descriptor: ReorderDescriptor, peer: PeerLease, packet: P, now: ReorderTick) -> Result<(), (ReorderError, P)> {
        let ReorderDescriptor { peer: index, tid, sequence, route } = descriptor;
        if route == ReorderRoute::Immediate {
            let index = self.retain(packet, sequence)?;
            self.publish(index, PublicationKind::Deliver);
            return Ok(());
        }
        if index != Some(peer.index()) {
            let index = self.retain(packet, sequence)?;
            self.publish(index, PublicationKind::Discard);
            return Ok(());
        }
        match route {
            ReorderRoute::Reorder => self.push(peer, tid, sequence, packet, now),
            ReorderRoute::Advance => {
                let index = self.retain(packet, sequence)?;
                self.publish(index, PublicationKind::Discard);
                self.advance(peer, tid, sequence, now);
                Ok(())
            }
            ReorderRoute::Immediate => unreachable!(),
        }
    }
}

#[cfg(test)]
#[path = "vendor_wireless_station_driver_receive_reorder_tests.rs"]
mod tests;
