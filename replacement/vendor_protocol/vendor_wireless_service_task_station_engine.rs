use super::*;

pub struct StationServiceRuntime {
    pub sockets: SocketServiceRuntime,
    now: TransportTick,
    publications: [Option<SocketPublication>; 16],
    publication_read: usize,
    publication_length: usize,
}
impl Default for StationServiceRuntime { fn default() -> Self { Self::new() } }
impl StationServiceRuntime {
    pub const fn new() -> Self {
        Self { sockets: SocketServiceRuntime::new(), now: TransportTick(0), publications: [None; 16], publication_read: 0, publication_length: 0 }
    }
    pub fn initialize(&mut self, socket: &mut SocketTaskRuntime, seed: TcpSequence) { socket.set_sequence_seed(seed); }
    pub fn reset(&mut self, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<(), StationError> {
        self.sockets.reset(discovery)?;
        self.publications = [None; 16];
        self.publication_read = 0;
        self.publication_length = 0;
        discovery.send_timer.cancel();
        Ok(())
    }
    pub fn publication(&mut self) -> Option<SocketPublication> {
        if self.publication_length == 0 { return None; }
        let value = self.publications[self.publication_read].take();
        self.publication_read = (self.publication_read + 1) % self.publications.len();
        self.publication_length -= 1;
        value
    }
    fn publish(&mut self, slot: SlotId, enabled: bool, discovery: &NetworkDiscoveryRuntime<'_>) -> Result<(), StationError> {
        if self.publication_length == self.publications.len() { return Err(StationError::QueueFull); }
        if let Some(value) = self.sockets.select_socket(slot, enabled, discovery.active_peer_count()) {
            let index = (self.publication_read + self.publication_length) % self.publications.len();
            self.publications[index] = Some(value);
            self.publication_length += 1;
        }
        Ok(())
    }
    fn open(&mut self, slot: SlotId, socket: &mut SocketTaskRuntime) -> Result<(), StationError> {
        self.sockets.open_default(slot, socket)?;
        self.sockets.set_phase(slot, SocketPhase::AwaitingOpen);
        Ok(())
    }
    fn close(&mut self, slot: SlotId, socket: &mut SocketTaskRuntime) -> Result<(), StationError> {
        self.sockets.close(slot, socket)?;
        self.sockets.set_phase(slot, SocketPhase::Opening);
        Ok(())
    }
    pub fn start(&mut self, slot: SlotId, socket: &mut SocketTaskRuntime) -> Result<bool, StationError> {
        match self.sockets.phase(slot) {
            SocketPhase::Inactive => { self.open(slot, socket)?; Ok(true) }
            SocketPhase::AwaitingOpen => Ok(true),
            _ => Ok(false),
        }
    }
    pub fn stop(&mut self, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        for index in 0..10 {
            let slot = SlotId(index);
            if self.stop_slot(slot, 0x6603, socket, discovery)? { return Ok(true); }
            self.sockets.slots[slot.index()].configured = false;
        }
        Ok(false)
    }
    fn stop_slot(&mut self, slot: SlotId, event: u16, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        match self.sockets.phase(slot) {
            SocketPhase::Inactive => Ok(false),
            SocketPhase::Opening => Ok(true),
            SocketPhase::AwaitingOpen => { self.close(slot, socket)?; Ok(true) }
            SocketPhase::Connected | SocketPhase::Transferring => {
                if self.transfer(slot, event, None, socket, discovery)? { return Ok(true); }
                self.close(slot, socket)?;
                Ok(true)
            }
        }
    }
    pub fn configure(&mut self, slot: SlotId, configuration: SocketConfiguration, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        let previous = self.sockets.slots[slot.index()].configuration;
        let changed = previous != configuration;
        let active = self.sockets.slots[slot.index()].configured;
        self.sockets.configure_slot(slot, configuration);
        discovery.bind_peer_address(slot, super::super::discovery::PeerMac(self.sockets.slots[slot.index()].identity.address));
        if !active && configuration.words[2] == 1 {
            let result = self.start(slot, socket)?;
            self.sockets.slots[slot.index()].configured = true;
            Ok(result)
        } else if active && configuration.words[2] == 0 {
            if changed && self.sockets.transfer_phase(slot) == TransferPhase::Responding { self.publish(slot, false, discovery)?; }
            if self.sockets.transfer_phase(slot) != TransferPhase::Idle { self.publish(slot, true, discovery)?; }
            let result = self.cancel(slot, socket, discovery)?;
            self.sockets.slots[slot.index()].configured = false;
            Ok(result)
        } else { Ok(false) }
    }
    pub fn cancel(&mut self, slot: SlotId, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        self.stop_slot(slot, 0x6622, socket, discovery)
    }
    fn validate_packet(message: &TaskMessage, discovery: &NetworkDiscoveryRuntime<'_>) -> Result<(), StationError> {
        let packet = message.packet.as_ref().ok_or(StationError::InvalidPayload)?;
        let length = message.payload.word(8).map_err(|_| StationError::InvalidPayload)? as usize;
        if discovery.packets.bytes(packet).map_err(StationError::Packet)?.len() != length { return Err(StationError::InvalidPayload); }
        Ok(())
    }
    pub fn process(&mut self, message: &mut TaskMessage, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        let kind = message.kind.0;
        if kind == 0x6603 { return self.stop(socket, discovery); }
        if kind == 0x660f {
            let mut handled = false;
            for index in 0..10 {
                let mut words = [0; 5];
                for (word, target) in words.iter_mut().enumerate() {
                    *target = message.payload.word(index as usize * 20 + word * 4).map_err(|_| StationError::InvalidPayload)?;
                }
                handled |= self.configure(SlotId(index), SocketConfiguration { words }, socket, discovery)?;
            }
            return Ok(handled);
        }
        let value = match kind {
            0x6610 | 0x6611 => message.payload.word(4).map_err(|_| StationError::InvalidPayload)?,
            0x6612 | 0x6614 => {
                let handle = message.payload.word(4).map_err(|_| StationError::InvalidPayload)?;
                match self.sockets.find_socket(handle) { Some(slot) => slot.get(), None => return Ok(false) }
            }
            0x6607 | 0x6613 => {
                let handle = message.payload.word(0).map_err(|_| StationError::InvalidPayload)?;
                match self.sockets.find_socket(handle) { Some(slot) => slot.get(), None => return Ok(false) }
            }
            _ => return Ok(false),
        };
        let slot = SlotId::new(value).ok_or(StationError::InvalidSlot)?;
        self.dispatch_slot(slot, kind, message, socket, discovery)
    }
    pub fn dispatch_slot(&mut self, slot: SlotId, kind: u16, message: &mut TaskMessage, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        let phase = self.sockets.phase(slot);
        let values = [message.payload.word(0), message.payload.word(4), message.payload.word(8)];
        let word = |offset: usize| values[offset / 4].map_err(|_| StationError::InvalidPayload);
        match (kind, phase) {
            (0x6603, _) => self.stop_slot(slot, 0x6603, socket, discovery),
            (0x6624, _) => self.cancel(slot, socket, discovery),
            (0x6623, _) => self.start(slot, socket),
            (0x6610, SocketPhase::Opening) => { self.sockets.close(slot, socket)?; Ok(true) }
            (0x6610, SocketPhase::AwaitingOpen) => {
                if word(0)? == 0 {
                    self.sockets.set_handle(slot, word(8)?);
                    discovery.bind_socket_handle(slot, super::super::discovery::LogicalSocketHandle(word(8)?));
                    self.sockets.set_phase(slot, SocketPhase::Connected);
                    Ok(false)
                } else { self.close(slot, socket)?; Ok(true) }
            }
            (0x6611, SocketPhase::Opening) => {
                if word(0)? as i32 == -50 { self.publish(slot, true, discovery)?; }
                self.sockets.set_phase(slot, SocketPhase::Inactive);
                Ok(false)
            }
            (0x6612, SocketPhase::Connected | SocketPhase::Transferring) => {
                if self.transfer(slot, 0x6622, None, socket, discovery)? { return Ok(true); }
                if self.sockets.slots[slot.index()].configuration.words[2] == 1 { self.open(slot, socket)?; Ok(true) }
                else { self.sockets.set_phase(slot, SocketPhase::Inactive); Ok(false) }
            }
            (0x6607, SocketPhase::Connected) => {
                Self::validate_packet(message, discovery)?;
                let packet = message.packet.as_ref().ok_or(StationError::InvalidPayload)?;
                let operation = self.sockets.send(slot, discovery.packets.bytes(packet).map_err(StationError::Packet)?, socket)?;
                discovery.bind_output_operation(slot, packet, operation);
                self.sockets.set_phase(slot, SocketPhase::Transferring);
                Ok(true)
            }
            (0x6607, SocketPhase::Transferring) => {
                Self::validate_packet(message, discovery)?;
                let packet = message.packet.take().ok_or(StationError::InvalidPayload)?;
                self.sockets.store_primary(slot, SocketResult { status: word(4)? as i32, value: word(8)? });
                if let Some(previous) = self.sockets.retain_primary_packet(slot, packet) { discovery.packets.release(previous).map_err(StationError::Packet)?; }
                Ok(true)
            }
            (0x6613, SocketPhase::Connected) => {
                Self::validate_packet(message, discovery)?;
                self.transfer(slot, kind, message.packet.take(), socket, discovery)
            }
            (0x6613, SocketPhase::Transferring) => {
                Self::validate_packet(message, discovery)?;
                let packet = message.packet.take().ok_or(StationError::InvalidPayload)?;
                self.sockets.store_secondary(slot, SocketResult { status: word(4)? as i32, value: word(8)? });
                if let Some(previous) = self.sockets.retain_secondary_packet(slot, packet) { discovery.packets.release(previous).map_err(StationError::Packet)?; }
                Ok(true)
            }
            (0x6614, SocketPhase::Transferring) => self.complete_send(slot, word(0)? as i32 == -50, socket, discovery),
            _ => Ok(false),
        }
    }
    fn complete_send(&mut self, slot: SlotId, timed_out: bool, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        if timed_out {
            self.sockets.clear_primary(slot);
            if let Some(packet) = self.sockets.take_primary_packet(slot) { discovery.packets.release(packet).map_err(StationError::Packet)?; }
        }
        if self.transfer(slot, 0x6614, None, socket, discovery)? { return Ok(true); }
        let pending = self.sockets.take_primary(slot);
        let packet = self.sockets.take_primary_packet(slot);
        if let Some(packet) = packet {
            let result = self.sockets.send(slot, discovery.packets.bytes(&packet).map_err(StationError::Packet)?, socket);
            if let Err(error) = result {
                self.sockets.store_primary(slot, pending);
                self.sockets.retain_primary_packet(slot, packet);
                return Err(error);
            }
            discovery.bind_output_operation(slot, &packet, result.expect("The send result was checked"));
            discovery.packets.release(packet).map_err(StationError::Packet)?;
            return Ok(true);
        }
        self.sockets.set_phase(slot, SocketPhase::Connected);
        self.sockets.take_secondary(slot);
        let packet = self.sockets.take_secondary_packet(slot);
        if packet.is_none() { return Ok(false); }
        self.transfer(slot, 0x6613, Some(packet.ok_or(StationError::InvalidPayload)?), socket, discovery)
    }
    pub fn receive_socket_completion(&mut self, completion: SocketTaskSocketCompletion, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        let Some((slot, result)) = self.sockets.completion(completion) else { return Ok(false); };
        match (result, self.sockets.phase(slot)) {
            (ServiceSocketCompletion::Opened(Ok(_)), SocketPhase::AwaitingOpen) => {
                self.sockets.set_handle(slot, slot.get());
                discovery.bind_socket_handle(slot, super::super::discovery::LogicalSocketHandle(slot.get()));
                self.sockets.set_phase(slot, SocketPhase::Connected);
                Ok(false)
            }
            (ServiceSocketCompletion::Opened(_), SocketPhase::AwaitingOpen) => { self.close(slot, socket)?; Ok(true) }
            (ServiceSocketCompletion::Opened(_), SocketPhase::Opening) => { self.sockets.close(slot, socket)?; Ok(true) }
            (ServiceSocketCompletion::Closed(status), SocketPhase::Opening) => {
                if status == Err(SocketTaskError::Cancelled) { self.publish(slot, true, discovery)?; }
                self.sockets.set_phase(slot, SocketPhase::Inactive);
                Ok(false)
            }
            (ServiceSocketCompletion::Sent(status), SocketPhase::Transferring) => self.complete_send(slot, status == Err(SocketTaskError::Cancelled), socket, discovery),
            _ => Ok(false),
        }
    }
    pub fn poll(&mut self, now: TransportTick, queue: &mut super::super::TaskQueue<'_>, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        self.now = now;
        discovery.poll(now, queue).map_err(StationError::Discovery)
    }
    fn transfer(&mut self, slot: SlotId, kind: u16, mut packet: Option<NetworkPacketLease>, socket: &mut SocketTaskRuntime, discovery: &mut NetworkDiscoveryRuntime<'_>) -> Result<bool, StationError> {
        let phase = self.sockets.transfer_phase(slot);
        let result = (|| { match (kind, phase) {
            (0x6603, TransferPhase::Ready) => Ok(true),
            (0x6603, TransferPhase::Responding) => {
                discovery.send_timer.arm(self.now, super::super::discovery::LogicalSocketHandle(self.sockets.handle(slot)));
                discovery.finish_response(slot, socket).map_err(StationError::Discovery)?;
                self.sockets.set_transfer_phase(slot, TransferPhase::Ready);
                Ok(true)
            }
            (0x6622, TransferPhase::Ready) => {
                discovery.send_timer.cancel();
                discovery.cancel().map_err(StationError::Discovery)?;
                discovery.drop_peer(slot);
                self.publish(slot, false, discovery)?;
                self.sockets.set_transfer_phase(slot, TransferPhase::Idle);
                Ok(false)
            }
            (0x6622, TransferPhase::Responding) => {
                discovery.drop_peer(slot);
                self.publish(slot, false, discovery)?;
                self.sockets.set_transfer_phase(slot, TransferPhase::Idle);
                Ok(false)
            }
            (0x6613, TransferPhase::Idle | TransferPhase::Responding) => {
                let lease = packet.as_ref().ok_or(StationError::InvalidPayload)?;
                let classification = discovery.classify_packet(slot, lease).map_err(StationError::Discovery)?;
                if classification.kind == TransferKind::Command {
                    if let Some(peer) = discovery.handshake() {
                        for (target, value) in self.sockets.slots[slot.index()].identity.name.iter_mut().zip(peer.name) { *target = value as u8; }
                    }
                }
                if phase == TransferPhase::Idle {
                    match classification.kind {
                        TransferKind::Invalid => discovery.send_response(slot, 0x2019, socket).map_err(StationError::Discovery)?,
                        TransferKind::Command => {
                            discovery.send_response(slot, classification.status, socket).map_err(StationError::Discovery)?;
                            if classification.status == 0 { self.sockets.set_transfer_phase(slot, TransferPhase::Responding); }
                        }
                        TransferKind::Other => {}
                    }
                } else if classification.kind == TransferKind::Invalid {
                    discovery.drop_peer(slot);
                    self.publish(slot, false, discovery)?;
                    self.sockets.set_transfer_phase(slot, TransferPhase::Idle);
                } else {
                    discovery.dispatch_packet_lease(slot, packet.take().ok_or(StationError::InvalidPayload)?, socket).map_err(StationError::Discovery)?;
                }
                Ok(false)
            }
            (0x6614, TransferPhase::Ready) => {
                discovery.cancel().map_err(StationError::Discovery)?;
                discovery.send_timer.cancel();
                discovery.drop_peer(slot);
                self.publish(slot, false, discovery)?;
                self.sockets.set_transfer_phase(slot, TransferPhase::Idle);
                Ok(false)
            }
            (0x6614, TransferPhase::Responding) => {
                discovery.cancel().map_err(StationError::Discovery)?;
                Ok(false)
            }
            _ => Ok(false),
        } })();
        if let Some(packet) = packet { discovery.packets.release(packet).map_err(StationError::Packet)?; }
        result
    }
}
