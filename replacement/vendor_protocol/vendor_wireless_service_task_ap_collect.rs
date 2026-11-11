use super::packets::{DeviceName, ServiceEntry, ServiceId, ServiceName};
use crate::vendor_wireless_network::{MacAddress, dhcp::Milliseconds, state::Ipv4Address};
use crate::vendor_wireless_service_task::station::SlotId;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionError { Busy, Inactive, UnknownClient, ClientCapacity, InvalidNotification }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AcceptedClient { pub slot: SlotId, pub address: Ipv4Address, pub hardware: MacAddress }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NotificationResponse { Accepted, Forbidden, None }
impl NotificationResponse {
    pub const fn bytes(self) -> &'static [u8] { match self { Self::Accepted => b"HTTP/1.1 200 OK\r\n", Self::Forbidden => b"HTTP/1.1 403 Forbidden\r\n", Self::None => b"" } }
}
#[derive(Clone, Copy)]
struct Client { peer: AcceptedClient, closed: bool, record: Option<ServiceEntry> }
pub struct ServiceBatch { records: [Option<ServiceEntry>; 6], length: usize }
impl ServiceBatch { pub fn entries(&self) -> impl Iterator<Item = ServiceEntry> + '_ { self.records[..self.length].iter().flatten().copied() } }
pub struct ServiceCollector { clients: [Option<Client>; 10], active: bool, deadline: Milliseconds, finished: usize, ready: bool }
impl ServiceCollector {
    pub const fn new() -> Self { Self { clients: [None; 10], active: false, deadline: Milliseconds(0), finished: 0, ready: false } }
    pub fn start(&mut self, now: Milliseconds) -> Result<(), CollectionError> {
        if self.active { return Err(CollectionError::Busy); }
        self.clients.fill(None); self.active = true; self.deadline = Milliseconds(now.0.wrapping_add(10_000)); self.finished = 0; self.ready = false; Ok(())
    }
    pub const fn active(&self) -> bool { self.active }
    pub fn cancel(&mut self) { self.active = false; self.clients.fill(None); self.finished = 0; self.ready = false; }
    pub fn accept(&mut self, peer: AcceptedClient) -> Result<(), CollectionError> {
        if !self.active { return Err(CollectionError::Inactive); }
        let index = peer.slot.get() as usize;
        if self.clients[index].is_some_and(|client| !client.closed) { return Err(CollectionError::ClientCapacity); }
        self.clients[index] = Some(Client { peer, closed: false, record: None }); Ok(())
    }
    pub fn receive(&mut self, slot: SlotId, bytes: &[u8]) -> Result<NotificationResponse, CollectionError> {
        if !self.active { return Err(CollectionError::Inactive); }
        let client = self.clients[slot.get() as usize].as_mut().ok_or(CollectionError::UnknownClient)?;
        if client.closed { return Err(CollectionError::UnknownClient); }
        let bytes = &bytes[..bytes.len().min(110)];
        if bytes.starts_with(b"HTTP/1.1 200 OK") || bytes.starts_with(b"HTTP/1.1 403 Fo") { return Ok(NotificationResponse::None); }
        if !bytes.starts_with(b"NOTIFY * HTTP/1") { client.record = None; return Err(CollectionError::InvalidNotification); }
        let Some(start) = bytes.windows(9).position(|window| window == b"IMPORTER:").map(|index| index + 9) else { return Ok(NotificationResponse::Forbidden); };
        let tail = bytes.get(start..).ok_or(CollectionError::InvalidNotification)?;
        let tail = tail.strip_prefix(b" ").unwrap_or(tail);
        let Some(end) = tail.windows(2).position(|window| window == b"\r\n") else { return Ok(NotificationResponse::Forbidden); };
        let name = match DeviceName::parse(&tail[..end]) { Ok(name) => name, Err(_) => return Ok(NotificationResponse::Forbidden) };
        let name = ServiceName::parse(name.as_bytes()).map_err(|_| CollectionError::InvalidNotification)?;
        client.record = Some(ServiceEntry { id: ServiceId(u32::from_le_bytes(client.peer.address.0)), name, address: client.peer.hardware });
        Ok(NotificationResponse::Accepted)
    }
    pub fn closed(&mut self, slot: SlotId) -> Result<(), CollectionError> {
        if !self.active { return Err(CollectionError::Inactive); }
        let client = self.clients[slot.get() as usize].as_mut().ok_or(CollectionError::UnknownClient)?;
        if !client.closed { client.closed = true; self.finished += 1; }
        if self.finished >= 6 { self.ready = true; }
        Ok(())
    }
    pub fn poll(&mut self, now: Milliseconds) -> Option<ServiceBatch> {
        if !self.active || (!self.ready && now.0.wrapping_sub(self.deadline.0) >= 0x8000_0000) { return None; }
        let mut batch = ServiceBatch { records: [None; 6], length: 0 };
        for record in self.clients.iter().flatten().filter_map(|client| client.record) {
            batch.records[batch.length] = Some(record); batch.length += 1;
            if batch.length == 6 { break; }
        }
        self.active = false; Some(batch)
    }
}
impl Default for ServiceCollector { fn default() -> Self { Self::new() } }

#[cfg(test)]
#[path = "vendor_wireless_service_task_ap_collect_tests.rs"]
mod tests;
