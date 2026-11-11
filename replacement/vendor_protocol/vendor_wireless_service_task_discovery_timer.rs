use super::DiscoveryError;
use super::super::{TaskMessage, TaskMessageKind, TaskPayload, TaskQueue};
use super::super::super::vendor_wireless_network::transport::TransportTick;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogicalSocketHandle(pub u32);

pub struct SendTimer {
    pending: Option<(TransportTick, LogicalSocketHandle)>,
}

impl SendTimer {
    pub const fn new() -> Self { Self { pending: None } }
    pub fn arm(&mut self, now: TransportTick, handle: LogicalSocketHandle) {
        self.pending = Some((TransportTick(now.0.wrapping_add(2000)), handle));
    }
    pub fn cancel(&mut self) { self.pending = None; }
    pub fn poll(&mut self, now: TransportTick, queue: &mut TaskQueue<'_>) -> Result<bool, DiscoveryError> {
        let Some((deadline, handle)) = self.pending else { return Ok(false); };
        if !now.reached(deadline) { return Ok(false); }
        let mut bytes = [0; 200];
        bytes[..4].copy_from_slice(&(-50i32).to_le_bytes());
        bytes[4..8].copy_from_slice(&handle.0.to_le_bytes());
        let payload = TaskPayload::parse(&bytes).map_err(DiscoveryError::Payload)?;
        let message = TaskMessage::new(TaskMessageKind(0x6614), payload);
        queue.enqueue(message).map_err(|_| DiscoveryError::QueueFull)?;
        self.pending = None;
        Ok(true)
    }
}
