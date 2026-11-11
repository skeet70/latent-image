//! X-M1 AP WPA authenticator, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.

use crate::vendor_wireless_network::{MacAddress, Ssid};
use xm1_wireless_crypto::zeroize::Zeroize;
use crate::wireless_association::station::crypto;
use crypto::random::{RandomPool, RtosClock};
use xm1_rtos_own_first::Machine;

#[path = "vendor_wireless_access_point_authenticator_elements.rs"]
mod elements;
pub use elements::{validate_association, InformationElement};

const PEERS: usize = crate::vendor_wireless_peers::PEER_CAPACITY;
const QUEUE: usize = 64;
const FRAME_CAPACITY: usize = 512;
const MIC: core::ops::Range<usize> = 81..97;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol { Wpa, Wpa2 }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cipher { Tkip, Ccmp }
impl Cipher {
    fn length(self) -> usize { match self { Self::Tkip => 32, Self::Ccmp => 16 } }
    fn version(self) -> u16 { match self { Self::Tkip => 1, Self::Ccmp => 2 } }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    InvalidCredential, InvalidElement, UnsupportedSecurity, UnknownPeer,
    PeerCapacity, QueueCapacity, NotReady, Busy, StaleCompletion, PhysicalFailure,
    InvalidFrame, InvalidMic, Replay, InvalidState, Crypto, CounterExhausted,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EffectId(u64);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Milliseconds(pub u64);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame { bytes: [u8; FRAME_CAPACITY], length: usize }
impl Frame { pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] } }
#[derive(Clone, Eq, PartialEq)]
pub struct Key { bytes: [u8; 32], length: usize }
impl Key { pub fn as_bytes(&self) -> &[u8] { &self.bytes[..self.length] } }
impl core::fmt::Debug for Key {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("Key([redacted])") }
}
impl Drop for Key { fn drop(&mut self) { self.bytes.zeroize(); } }
#[derive(Debug)]
pub struct Effect { pub id: EffectId, pub operation: Operation }
#[derive(Debug)]
pub enum Operation {
    Transmit { peer: MacAddress, frame: Frame, encrypt: bool },
    Install { peer: Option<MacAddress>, index: u8, cipher: Cipher, key: Key },
    Remove { peer: Option<MacAddress>, index: u8 },
    Authorize { peer: MacAddress },
    Unauthorize { peer: MacAddress },
    Deauthenticate { peer: MacAddress, reason: u16 },
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum State { Message2, Message4, Installing, Authorizing, Established, Group2 }
struct Peer {
    address: MacAddress,
    generation: u64,
    state: State,
    association_ie: [u8; 257],
    association_ie_length: usize,
    nonce: [u8; 32],
    keys: Option<crypto::PairwiseKeys>,
    replay: u64,
    pending_replay: [Option<u64>; 4],
    request_replay: Option<u64>,
    attempts: u8,
    deadline: Option<Milliseconds>,
    authorized: bool,
    has_group: bool,
}
#[derive(Clone, Copy)]
enum Completion { GroupInstall, Sent, PairwiseInstall, Authorized, Cleanup }
#[derive(Clone, Copy)]
struct Pending { id: EffectId, owner: Option<(MacAddress, u64)>, completion: Completion }
struct Command { effect: Effect, pending: Pending }

pub struct Authenticator {
    address: MacAddress,
    protocol: Protocol,
    cipher: Cipher,
    master: crypto::MasterKey,
    group_master: [u8; 32],
    counter: [u8; 32],
    group_nonce: [u8; 32],
    group_key: Key,
    group_index: u8,
    group_update_pending: bool,
    initialized: bool,
    group_install_queued: bool,
    ready: bool,
    stopped: bool,
    peers: [Option<Peer>; PEERS],
    queue: [Option<Command>; QUEUE],
    head: usize,
    length: usize,
    inflight: Option<Pending>,
    serial: u64,
    generation: u64,
    now: Milliseconds,
    next_group_rekey: Milliseconds,
    next_master_rekey: Milliseconds,
    mic_failure: Option<Milliseconds>,
    countermeasures_until: Option<Milliseconds>,
}
impl Authenticator {
    pub fn new(address: MacAddress, protocol: Protocol, cipher: Cipher, ssid: &Ssid, credential: &[u8]) -> Result<Self, Error> {
        let master = if credential.len() == 64 {
            let mut raw = [0; 32];
            for (out, pair) in raw.iter_mut().zip(credential.chunks_exact(2)) {
                *out = hex(pair[0]).ok_or(Error::InvalidCredential)? << 4 | hex(pair[1]).ok_or(Error::InvalidCredential)?;
            }
            crypto::MasterKey::from_bytes(raw)
        } else {
            if !(8..=63).contains(&credential.len()) || !credential.iter().all(|byte| (32..=126).contains(byte)) { return Err(Error::InvalidCredential); }
            crypto::derive_master_key(credential, ssid.as_bytes())
        };
        let this = Self { address, protocol, cipher, master, group_master: [0; 32], counter: [0; 32],
            group_nonce: [0; 32], group_key: Key { bytes: [0; 32], length: cipher.length() }, group_index: 1, group_update_pending: false,
            initialized: false, group_install_queued: false,
            ready: false, stopped: false, peers: core::array::from_fn(|_| None), queue: core::array::from_fn(|_| None),
            head: 0, length: 0, inflight: None, serial: 0, generation: 0, now: Milliseconds(0),
            next_group_rekey: Milliseconds(600_000), next_master_rekey: Milliseconds(86_400_000),
            mic_failure: None, countermeasures_until: None };
        Ok(this)
    }
    pub fn initialize<M: Machine>(&mut self, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        if self.initialized || self.stopped { return Err(Error::InvalidState); }
        self.now = Milliseconds(clock.sample().map_err(|_| Error::Crypto)?.0 as u64);
        self.next_group_rekey = Milliseconds(self.now.0.saturating_add(600_000));
        self.next_master_rekey = Milliseconds(self.now.0.saturating_add(86_400_000));
        random.generate(clock, &mut self.group_master).map_err(|_| Error::Crypto)?;
        random.generate(clock, &mut self.counter).map_err(|_| Error::Crypto)?;
        self.derive_group(random, clock)?;
        self.initialized = true;
        self.install_group()
    }
    pub fn is_ready(&self) -> bool { self.ready && !self.stopped }
    pub fn is_drained(&self) -> bool { self.length == 0 && self.inflight.is_none() }
    pub fn is_authorized(&self, peer: MacAddress) -> bool {
        self.is_ready() && self.peers.iter().flatten().any(|p| p.address == peer && p.authorized)
    }
    pub fn information_element(&self) -> elements::InformationElement { elements::advertisement(self.protocol, self.cipher) }
    fn index(&self, peer: MacAddress) -> Result<usize, Error> {
        self.peers.iter().position(|p| p.as_ref().is_some_and(|p| p.address == peer)).ok_or(Error::UnknownPeer)
    }
    fn owner(&self, index: usize) -> Option<(MacAddress, u64)> {
        self.peers[index].as_ref().map(|p| (p.address, p.generation))
    }
    fn live(&self, owner: Option<(MacAddress, u64)>) -> bool {
        owner.is_none_or(|(address, generation)| self.peers.iter().flatten().any(|p| p.address == address && p.generation == generation))
    }
    fn enqueue(&mut self, operation: Operation, owner: Option<(MacAddress, u64)>, completion: Completion) -> Result<(), Error> {
        if self.length == QUEUE { return Err(Error::QueueCapacity); }
        self.serial = self.serial.checked_add(1).ok_or(Error::CounterExhausted)?;
        let id = EffectId(self.serial);
        let slot = (self.head + self.length) % QUEUE;
        self.queue[slot] = Some(Command { effect: Effect { id, operation }, pending: Pending { id, owner, completion } });
        self.length += 1;
        Ok(())
    }
    pub fn next_effect(&mut self) -> Option<Effect> {
        if self.inflight.is_some() { return None; }
        while self.length != 0 {
            let command = self.queue[self.head].take()?;
            self.head = (self.head + 1) % QUEUE; self.length -= 1;
            if !self.live(command.pending.owner) { continue; }
            self.inflight = Some(command.pending);
            return Some(command.effect);
        }
        None
    }
    pub fn effect_complete(&mut self, id: EffectId, success: bool) -> Result<(), Error> {
        let pending = self.inflight.ok_or(Error::StaleCompletion)?;
        if pending.id != id { return Err(Error::StaleCompletion); }
        self.inflight = None;
        if !self.live(pending.owner) { return Err(Error::StaleCompletion); }
        if !success {
            if let Some((peer, _)) = pending.owner { self.disconnect_reason(peer, 15)?; }
            else { self.cancel()?; }
            return Err(Error::PhysicalFailure);
        }
        match pending.completion {
            Completion::GroupInstall => { self.ready = !self.stopped; self.group_update_pending = false; self.group_install_queued = false; }
            Completion::PairwiseInstall => {
                let (address, _) = pending.owner.ok_or(Error::InvalidState)?;
                let index = self.index(address)?;
                let peer = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
                peer.state = State::Authorizing;
                self.enqueue(Operation::Authorize { peer: address }, pending.owner, Completion::Authorized)?;
            }
            Completion::Authorized => {
                let (address, _) = pending.owner.ok_or(Error::InvalidState)?;
                let index = self.index(address)?;
                let peer = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
                peer.authorized = true; peer.state = State::Established;
                if self.protocol == Protocol::Wpa { self.send_group(index)?; }
            }
            Completion::Sent => {
                let (address, _) = pending.owner.ok_or(Error::InvalidState)?;
                let index = self.index(address)?;
                let peer = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
                if matches!(peer.state, State::Message2 | State::Message4 | State::Group2) {
                    peer.deadline = Some(Milliseconds(self.now.0.saturating_add(1000)));
                }
            }
            Completion::Cleanup => {}
        }
        Ok(())
    }
    fn derive_group<M: Machine>(&mut self, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        self.group_nonce = self.counter;
        increment(&mut self.counter)?;
        let mut seed = [0; 62]; seed[..6].copy_from_slice(&self.address.0); seed[6..38].copy_from_slice(&self.group_nonce);
        let time = clock.sample().map_err(|_| Error::Crypto)?.0;
        seed[38..42].copy_from_slice(&(time / 1000).to_be_bytes());
        seed[42..46].copy_from_slice(&((time % 1000) * 1000).to_be_bytes());
        random.generate(clock, &mut seed[46..]).map_err(|_| Error::Crypto)?;
        prf(&self.group_master, b"Group key expansion", &seed, &mut self.group_key.bytes[..self.cipher.length()]);
        seed.zeroize();
        Ok(())
    }
    fn install_group(&mut self) -> Result<(), Error> {
        if self.group_install_queued { return Ok(()); }
        self.group_install_queued = true;
        self.enqueue(Operation::Install { peer: None, index: self.group_index, cipher: self.cipher, key: self.group_key.clone() }, None, Completion::GroupInstall)
    }
    pub fn associated(&mut self, address: MacAddress, ies: &[u8], now: Milliseconds) -> Result<(), Error> {
        self.now = now;
        if !self.is_ready() || self.group_update_pending || self.countermeasures_until.is_some_and(|until| now.0 < until.0) { return Err(Error::NotReady); }
        let ie = validate_association(self.protocol, self.cipher, ies)?;
        if self.index(address).is_ok() { self.disconnected(address)?; }
        let index = self.peers.iter().position(Option::is_none).ok_or(Error::PeerCapacity)?;
        self.generation = self.generation.checked_add(1).ok_or(Error::CounterExhausted)?;
        let mut association_ie = [0; 257]; association_ie[..ie.len()].copy_from_slice(ie);
        let nonce = self.counter; increment(&mut self.counter)?;
        self.peers[index] = Some(Peer { address, generation: self.generation, state: State::Message2, association_ie,
            association_ie_length: ie.len(), nonce, keys: None, replay: 0, pending_replay: [None; 4], request_replay: None,
            attempts: 0, deadline: None, authorized: false, has_group: false });
        self.enqueue(Operation::Unauthorize { peer: address }, self.owner(index), Completion::Cleanup)?;
        self.enqueue(Operation::Remove { peer: Some(address), index: 0 }, self.owner(index), Completion::Cleanup)?;
        self.send_pairwise(index, false)
    }
    fn send_pairwise(&mut self, index: usize, third: bool) -> Result<(), Error> {
        let mut data = [0; 320];
        let mut length = 0;
        if third {
            let ie = self.information_element(); data[..ie.as_bytes().len()].copy_from_slice(ie.as_bytes()); length = ie.as_bytes().len();
            if self.protocol == Protocol::Wpa2 { length += self.group_kde(&mut data[length..]); }
        }
        let flags = if third { 0x1c8 | if self.protocol == Protocol::Wpa2 { 0x200 } else { 0 } } else { 0x88 };
        let nonce = self.peers[index].as_ref().ok_or(Error::UnknownPeer)?.nonce;
        self.send(index, flags, &nonce, &data[..length], third && self.protocol == Protocol::Wpa2)
    }
    fn group_kde(&self, out: &mut [u8]) -> usize {
        let n = self.cipher.length(); out[..8].copy_from_slice(&[0xdd, (6 + n) as u8, 0, 0x0f, 0xac, 1, self.group_index, 0]);
        out[8..8+n].copy_from_slice(self.group_key.as_bytes()); 8 + n
    }
    fn send_group(&mut self, index: usize) -> Result<(), Error> {
        let mut data = [0; 40];
        let n = if self.protocol == Protocol::Wpa2 { self.group_kde(&mut data) } else {
            let n = self.cipher.length(); data[..n].copy_from_slice(self.group_key.as_bytes()); n
        };
        let peer = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
        if peer.state != State::Group2 { peer.attempts = 0; peer.pending_replay.fill(None); }
        peer.state = State::Group2;
        let nonce = self.group_nonce;
        self.send(index, 0x380, &nonce, &data[..n], true)
    }
    fn send(&mut self, index: usize, flags: u16, nonce: &[u8; 32], data: &[u8], encrypt: bool) -> Result<(), Error> {
        let peer = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
        peer.attempts += 1;
        if peer.attempts > 4 { let address = peer.address; return self.disconnect_reason(address, 15); }
        let mut frame = Frame { bytes: [0; FRAME_CAPACITY], length: 99 + data.len() };
        frame.bytes[..5].copy_from_slice(&[2, 3, 0, 0, if self.protocol == Protocol::Wpa2 { 2 } else { 254 }]);
        let mut info = flags | self.cipher.version();
        if encrypt && self.protocol == Protocol::Wpa2 { info |= 0x1000; }
        if self.protocol == Protocol::Wpa && flags & 8 == 0 { info |= (self.group_index as u16) << 4; }
        frame.bytes[5..7].copy_from_slice(&info.to_be_bytes());
        frame.bytes[7..9].copy_from_slice(&(self.cipher.length() as u16).to_be_bytes());
        peer.replay = peer.replay.checked_add(1).ok_or(Error::CounterExhausted)?;
        peer.pending_replay.rotate_right(1); peer.pending_replay[0] = Some(peer.replay);
        frame.bytes[9..17].copy_from_slice(&peer.replay.to_be_bytes()); frame.bytes[17..49].copy_from_slice(nonce);
        let data_length = if encrypt {
            let keys = peer.keys.as_ref().ok_or(Error::InvalidState)?;
            if self.cipher == Cipher::Ccmp {
                let mut padded = [0; 328]; padded[..data.len()].copy_from_slice(data);
                let n = (data.len() + 7) & !7;
                if n != data.len() { padded[data.len()] = 0xdd; }
                crypto::wrap_key_data(keys.encryption(), &padded[..n], &mut frame.bytes[99..]).map_err(|_| Error::Crypto)?
            } else {
                frame.bytes[49..65].copy_from_slice(&self.counter[16..]); increment(&mut self.counter)?;
                let mut rc4_key = [0; 32]; rc4_key[..16].copy_from_slice(&frame.bytes[49..65]); rc4_key[16..].copy_from_slice(keys.encryption());
                frame.bytes[99..99+data.len()].copy_from_slice(data);
                crypto::legacy::rc4_skip(&rc4_key, 256, &mut frame.bytes[99..99+data.len()]).map_err(|_| Error::Crypto)?;
                rc4_key.zeroize(); data.len()
            }
        } else { frame.bytes[99..99+data.len()].copy_from_slice(data); data.len() };
        frame.length = 99 + data_length;
        frame.bytes[2..4].copy_from_slice(&((frame.length - 4) as u16).to_be_bytes());
        frame.bytes[97..99].copy_from_slice(&(data_length as u16).to_be_bytes());
        if flags & 0x100 != 0 {
            let keys = peer.keys.as_ref().ok_or(Error::InvalidState)?;
            let mic = calculate_mic(self.cipher, keys.confirmation(), frame.as_bytes()); frame.bytes[MIC].copy_from_slice(&mic);
        }
        let address = peer.address; let encrypt = peer.authorized;
        self.enqueue(Operation::Transmit { peer: address, frame, encrypt }, self.owner(index), Completion::Sent)
    }
    pub fn receive_eapol<M: Machine>(&mut self, address: MacAddress, bytes: &[u8], now: Milliseconds,
        random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        self.now = now;
        let index = self.index(address)?;
        let frame = Parsed::parse(self.protocol, self.cipher, bytes)?;
        let peer = self.peers[index].as_mut().ok_or(Error::UnknownPeer)?;
        if frame.info & 0x800 != 0 {
            if peer.request_replay.is_some_and(|n| frame.replay <= n) { return Err(Error::Replay); }
            let keys = peer.keys.as_ref().ok_or(Error::InvalidState)?;
            verify_mic(self.cipher, keys.confirmation(), frame.bytes)?;
            peer.request_replay = Some(frame.replay);
            if frame.info & 0x400 != 0 && self.cipher == Cipher::Tkip { return self.michael_failure(now, random, clock); }
            if frame.info & 8 != 0 {
                peer.state = State::Message2; peer.attempts = 0; peer.pending_replay.fill(None);
                peer.keys = None; peer.deadline = None;
                peer.nonce = self.counter; increment(&mut self.counter)?;
                return self.send_pairwise(index, false);
            }
            return self.rekey_group(random, clock);
        }
        if !peer.pending_replay.contains(&Some(frame.replay)) { return Err(Error::Replay); }
        match peer.state {
            State::Message2 if frame.info & 8 != 0 && !frame.data.is_empty() => {
                let ie = validate_association(self.protocol, self.cipher, frame.data)?;
                if ie != &peer.association_ie[..peer.association_ie_length] { return self.disconnect_reason(address, 17); }
                let keys = crypto::derive_pairwise_keys(&self.master, &self.address.0, &address.0, &peer.nonce, frame.nonce);
                verify_mic(self.cipher, keys.confirmation(), frame.bytes)?;
                peer.keys = Some(keys); peer.state = State::Message4; peer.attempts = 0;
                peer.deadline = None; peer.pending_replay.fill(None);
                self.send_pairwise(index, true)
            }
            State::Message4 if frame.info & 8 != 0 && frame.data.is_empty() => {
                let keys = peer.keys.as_ref().ok_or(Error::InvalidState)?;
                verify_mic(self.cipher, keys.confirmation(), frame.bytes)?;
                let mut key = Key { bytes: [0; 32], length: self.cipher.length() };
                if self.cipher == Cipher::Tkip { key.bytes.copy_from_slice(keys.temporal_tkip()); }
                else { key.bytes[..16].copy_from_slice(keys.temporal()); }
                peer.state = State::Installing; peer.pending_replay.fill(None); peer.deadline = None;
                peer.has_group = self.protocol == Protocol::Wpa2;
                self.enqueue(Operation::Install { peer: Some(address), index: 0, cipher: self.cipher, key }, self.owner(index), Completion::PairwiseInstall)
            }
            State::Group2 if frame.info & 8 == 0 => {
                verify_mic(self.cipher, peer.keys.as_ref().ok_or(Error::InvalidState)?.confirmation(), frame.bytes)?;
                peer.state = State::Established; peer.has_group = true; peer.pending_replay.fill(None); peer.deadline = None;
                self.finish_group_update()
            }
            _ => Err(Error::InvalidState),
        }
    }
    pub fn tick<M: Machine>(&mut self, now: Milliseconds, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        self.now = now;
        if !self.is_ready() { return Ok(()); }
        if now.0 >= self.next_master_rekey.0 {
            random.generate(clock, &mut self.group_master).map_err(|_| Error::Crypto)?;
            self.next_master_rekey = Milliseconds(now.0.saturating_add(86_400_000));
        }
        if now.0 >= self.next_group_rekey.0 && !self.group_update_pending && self.peers.iter().flatten().all(|p| p.state == State::Established) {
            self.rekey_group(random, clock)?;
            self.next_group_rekey = Milliseconds(now.0.saturating_add(600_000));
        }
        for index in 0..PEERS {
            let Some(peer) = self.peers[index].as_mut() else { continue; };
            if !peer.deadline.is_some_and(|deadline| deadline.0 <= now.0) { continue; }
            peer.deadline = None;
            match peer.state {
                State::Message2 => self.send_pairwise(index, false)?,
                State::Message4 => self.send_pairwise(index, true)?,
                State::Group2 => self.send_group(index)?,
                _ => {}
            }
        }
        Ok(())
    }
    pub fn rekey_group<M: Machine>(&mut self, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        if !self.is_ready() { return Err(Error::NotReady); }
        if self.group_update_pending || self.peers.iter().flatten().any(|p| p.state != State::Established) { return Err(Error::Busy); }
        self.derive_group(random, clock)?;
        self.group_index = if self.group_index == 1 { 2 } else { 1 };
        self.group_update_pending = true;
        for index in 0..PEERS {
            if self.peers[index].as_ref().is_some_and(|p| p.authorized) { self.send_group(index)?; }
        }
        self.finish_group_update()
    }
    fn finish_group_update(&mut self) -> Result<(), Error> {
        if self.group_update_pending && !self.peers.iter().flatten().any(|peer| peer.state == State::Group2) {
            self.install_group()?;
        }
        Ok(())
    }
    pub fn disconnected(&mut self, address: MacAddress) -> Result<(), Error> {
        let index = self.index(address)?;
        self.peers[index] = None;
        self.enqueue(Operation::Unauthorize { peer: address }, None, Completion::Cleanup)?;
        self.enqueue(Operation::Remove { peer: Some(address), index: 0 }, None, Completion::Cleanup)?;
        self.finish_group_update()
    }
    fn disconnect_reason(&mut self, address: MacAddress, reason: u16) -> Result<(), Error> {
        self.disconnected(address)?;
        self.enqueue(Operation::Deauthenticate { peer: address, reason }, None, Completion::Cleanup)
    }
    pub fn cancel(&mut self) -> Result<(), Error> {
        self.ready = false; self.stopped = true;
        let queued = self.length;
        for _ in 0..queued {
            let command = self.queue[self.head].take();
            self.head = (self.head + 1) % QUEUE;
            self.length -= 1;
            if let Some(command) = command {
                if command.pending.owner.is_none() && matches!(command.pending.completion, Completion::Cleanup) {
                    let tail = (self.head + self.length) % QUEUE;
                    self.queue[tail] = Some(command);
                    self.length += 1;
                }
            }
        }
        self.group_update_pending = false;
        for index in 0..PEERS {
            if let Some(address) = self.peers[index].as_ref().map(|p| p.address) { self.disconnect_reason(address, 3)?; }
        }
        for index in 1..=2 { self.enqueue(Operation::Remove { peer: None, index }, None, Completion::Cleanup)?; }
        self.group_master.zeroize(); self.counter.zeroize(); self.group_key.bytes.zeroize();
        Ok(())
    }
    pub fn michael_failure<M: Machine>(&mut self, now: Milliseconds, random: &mut RandomPool, clock: &RtosClock<'_, M>) -> Result<(), Error> {
        if self.cipher != Cipher::Tkip { return Ok(()); }
        if self.mic_failure.is_some_and(|last| now.0.saturating_sub(last.0) < 60_000) {
            self.countermeasures_until = Some(Milliseconds(now.0.saturating_add(60_000)));
            for index in 0..PEERS {
                if let Some(address) = self.peers[index].as_ref().map(|p| p.address) { self.disconnect_reason(address, 14)?; }
            }
            self.rekey_group(random, clock)?;
        }
        self.mic_failure = Some(now);
        Ok(())
    }
}
impl Drop for Authenticator {
    fn drop(&mut self) { self.group_master.zeroize(); self.counter.zeroize(); self.group_nonce.zeroize(); }
}
fn hex(byte: u8) -> Option<u8> { match byte { b'0'..=b'9' => Some(byte-b'0'), b'a'..=b'f' => Some(byte-b'a'+10), b'A'..=b'F' => Some(byte-b'A'+10), _ => None } }
fn increment(bytes: &mut [u8; 32]) -> Result<(), Error> {
    if bytes.iter().all(|byte| *byte == 255) { return Err(Error::CounterExhausted); }
    for byte in bytes.iter_mut().rev() { let (next, carry) = byte.overflowing_add(1); *byte = next; if !carry { return Ok(()); } }
    Err(Error::CounterExhausted)
}
fn prf(key: &[u8], label: &[u8], data: &[u8], out: &mut [u8]) {
    for (counter, chunk) in out.chunks_mut(20).enumerate() {
        let digest = crypto::hmac_sha1(key, &[label, &[0], data, &[counter as u8]]); chunk.copy_from_slice(&digest[..chunk.len()]);
    }
}
fn calculate_mic(cipher: Cipher, key: &[u8], frame: &[u8]) -> [u8; 16] {
    let fragments: &[&[u8]] = &[&frame[..81], &[0; 16], &frame[97..]];
    match cipher {
        Cipher::Tkip => crypto::legacy::hmac_md5(key, fragments),
        Cipher::Ccmp => { let digest = crypto::hmac_sha1(key, fragments); let mut mic = [0; 16]; mic.copy_from_slice(&digest[..16]); mic }
    }
}
fn verify_mic(cipher: Cipher, key: &[u8], frame: &[u8]) -> Result<(), Error> {
    if crypto::equal_authenticator(&calculate_mic(cipher, key, frame), &frame[MIC]) { Ok(()) } else { Err(Error::InvalidMic) }
}
struct Parsed<'a> { bytes: &'a [u8], info: u16, replay: u64, nonce: &'a [u8; 32], data: &'a [u8] }
impl<'a> Parsed<'a> {
    fn parse(protocol: Protocol, cipher: Cipher, input: &'a [u8]) -> Result<Self, Error> {
        if input.len() < 99 || input[1] != 3 { return Err(Error::InvalidFrame); }
        let length = u16::from_be_bytes([input[2], input[3]]) as usize + 4;
        if length < 99 || length > input.len() { return Err(Error::InvalidFrame); }
        let bytes = &input[..length];
        if bytes[4] != if protocol == Protocol::Wpa2 { 2 } else { 254 } { return Err(Error::InvalidFrame); }
        let info = u16::from_be_bytes([bytes[5], bytes[6]]);
        if info & 7 != cipher.version() || info & 0x80 != 0 || info & 0x100 == 0 || info & 0x2000 != 0 { return Err(Error::InvalidFrame); }
        let n = u16::from_be_bytes([bytes[97], bytes[98]]) as usize;
        if n > length - 99 { return Err(Error::InvalidFrame); }
        Ok(Self { bytes, info, replay: u64::from_be_bytes(bytes[9..17].try_into().map_err(|_| Error::InvalidFrame)?),
            nonce: bytes[17..49].try_into().map_err(|_| Error::InvalidFrame)?, data: &bytes[99..99+n] })
    }
}

#[cfg(test)]
#[path = "vendor_wireless_access_point_authenticator_tests.rs"]
mod tests;
