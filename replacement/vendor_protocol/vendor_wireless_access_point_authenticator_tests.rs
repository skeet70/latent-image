use super::{Authenticator, Cipher, Effect, Error, Milliseconds, Operation, Protocol, validate_association};
use crate::vendor_wireless_network::{MacAddress, Ssid};
use crate::wireless_association::station::crypto::{self, random::{ClockScratch, RandomPool, RtosClock}};
use xm1_rtos_own_first::{Cpsr, DebugIdentity, Domain, GuestAddress, Machine};
use core::cell::Cell;

const AP: MacAddress = MacAddress([2,0,0,0,0,1]);
const STA: MacAddress = MacAddress([2,0,0,0,0,2]);
const SCRATCH: GuestAddress = GuestAddress(0x1000);
const RSN: &[u8] = &[48,20,1,0,0,15,172,4,1,0,0,15,172,4,1,0,0,15,172,2,0,0];
const WPA: &[u8] = &[221,24,0,80,242,1,1,0,0,80,242,2,1,0,0,80,242,2,1,0,0,80,242,2,0,0];

struct Memory { ticks: Cell<u32>, scratch: Cell<u32>, cpsr: Cell<Cpsr> }
impl Memory {
    fn new() -> Self { Self { ticks: Cell::new(12345), scratch: Cell::new(0), cpsr: Cell::new(Cpsr(0x13)) } }
    fn clock(&self) -> RtosClock<'_, Self> { RtosClock::new(self, Domain::A, ClockScratch::new(SCRATCH).unwrap()) }
}
impl Machine for Memory {
    fn load_word(&self, at: GuestAddress) -> u32 {
        if at == SCRATCH { self.scratch.get() }
        else if at == Domain::A.words().timer_base { let now = self.ticks.get(); self.ticks.set(now + 1); now }
        else { 0 }
    }
    fn load_byte(&self, _: GuestAddress) -> u8 { 0 }
    fn store_word(&self, at: GuestAddress, value: u32) { assert_eq!(at, SCRATCH); self.scratch.set(value); }
    fn cpsr(&self) -> Cpsr { self.cpsr.get() }
    fn write_cpsr_control(&self, value: Cpsr) { self.cpsr.set(value); }
    fn debug_identity(&self) -> DebugIdentity { DebugIdentity::PRIMARY }
}
fn auth(protocol: Protocol, cipher: Cipher) -> Authenticator {
    Authenticator::new(AP, protocol, cipher, &Ssid::parse(b"IEEE").unwrap(), b"password").unwrap()
}
fn ready(protocol: Protocol, cipher: Cipher, random: &mut RandomPool, memory: &Memory) -> (Authenticator, [u8; 32], usize) {
    let mut ap = auth(protocol, cipher);
    assert!(!ap.is_ready());
    ap.initialize(random, &memory.clock()).unwrap();
    let effect = ap.next_effect().unwrap();
    let mut installed = [0; 32];
    let length = match &effect.operation {
        Operation::Install { peer: None, index: 1, key, .. } => { installed[..key.as_bytes().len()].copy_from_slice(key.as_bytes()); key.as_bytes().len() }
        _ => panic!("expected initial GTK installation"),
    };
    assert!(!ap.is_ready());
    ap.effect_complete(effect.id, true).unwrap();
    assert!(ap.is_ready());
    (ap, installed, length)
}
fn next_tx(ap: &mut Authenticator) -> Effect {
    loop {
        let effect = ap.next_effect().expect("transmit effect");
        if matches!(effect.operation, Operation::Transmit { .. }) { return effect; }
        assert!(matches!(effect.operation, Operation::Remove { .. } | Operation::Unauthorize { .. }));
        ap.effect_complete(effect.id, true).unwrap();
    }
}
fn response(protocol: Protocol, cipher: Cipher, tx: &[u8], keys: &crypto::PairwiseKeys, nonce: &[u8;32], data: &[u8], pairwise: bool) -> ([u8; 512], usize) {
    let mut out = [0; 512]; let n = 99 + data.len();
    out[..5].copy_from_slice(&[1,3,0,0,if protocol == Protocol::Wpa2 { 2 } else { 254 }]);
    out[2..4].copy_from_slice(&((n - 4) as u16).to_be_bytes());
    let version: u16 = if cipher == Cipher::Ccmp { 2 } else { 1 };
    let flags = version | 0x100 | if pairwise { 8 } else { 0x200 };
    out[5..7].copy_from_slice(&flags.to_be_bytes()); out[9..17].copy_from_slice(&tx[9..17]); out[17..49].copy_from_slice(nonce);
    out[97..99].copy_from_slice(&(data.len() as u16).to_be_bytes()); out[99..n].copy_from_slice(data);
    let mut signature = [0;16];
    if cipher == Cipher::Ccmp { signature.copy_from_slice(&crypto::hmac_sha1(keys.confirmation(), &[&out[..n]])[..16]); }
    else { signature = crypto::legacy::hmac_md5(keys.confirmation(), &[&out[..n]]); }
    out[81..97].copy_from_slice(&signature); (out,n)
}
fn begin(ap: &mut Authenticator, protocol: Protocol, cipher: Cipher, ie: &[u8], random: &mut RandomPool, memory: &Memory) -> (crypto::PairwiseKeys, [u8;32]) {
    ap.associated(STA, ie, Milliseconds(0)).unwrap();
    let first = next_tx(ap);
    let Operation::Transmit { frame, peer, encrypt } = &first.operation else { unreachable!() };
    assert_eq!(*peer, STA); assert!(!encrypt);
    assert_eq!(u16::from_be_bytes([frame.as_bytes()[5],frame.as_bytes()[6]]) & 0x1f8, 0x88);
    let anonce: [u8;32] = frame.as_bytes()[17..49].try_into().unwrap();
    let snonce = [0x39;32];
    let master = crypto::derive_master_key(b"password", b"IEEE");
    let keys = crypto::derive_pairwise_keys(&master, &AP.0, &STA.0, &anonce, &snonce);
    let (bytes,n) = response(protocol,cipher,frame.as_bytes(),&keys,&snonce,ie,true);
    ap.effect_complete(first.id,true).unwrap();
    ap.receive_eapol(STA,&bytes[..n],Milliseconds(1),random,&memory.clock()).unwrap();
    (keys,snonce)
}
#[test]
fn wpa2_keys_reach_driver_before_peer_authorization() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,gtk,gtk_length) = ready(Protocol::Wpa2,Cipher::Ccmp,&mut random,&memory);
    let (keys,nonce) = begin(&mut ap,Protocol::Wpa2,Cipher::Ccmp,RSN,&mut random,&memory);
    let third = next_tx(&mut ap);
    let Operation::Transmit { frame, .. } = &third.operation else { unreachable!() };
    let wire = frame.as_bytes();
    assert_eq!(u16::from_be_bytes([wire[5],wire[6]]) & 0x13f8,0x13c8);
    let mut plain = [0;128]; let length = crypto::unwrap_key_data(keys.encryption(),&wire[99..],&mut plain).unwrap();
    assert!(length >= RSN.len() + 8 + gtk_length);
    assert_eq!(&plain[..RSN.len()], RSN);
    assert_eq!(&plain[RSN.len()+8..RSN.len()+8+gtk_length],&gtk[..gtk_length]);
    let (fourth,n) = response(Protocol::Wpa2,Cipher::Ccmp,wire,&keys,&nonce,&[],true);
    ap.effect_complete(third.id,true).unwrap();
    ap.receive_eapol(STA,&fourth[..n],Milliseconds(2),&mut random,&memory.clock()).unwrap();
    assert!(!ap.is_authorized(STA));
    let install = ap.next_effect().unwrap();
    let Operation::Install { peer: Some(peer), index: 0, key, .. } = &install.operation else { panic!("pairwise installation") };
    assert_eq!(*peer,STA); assert_eq!(key.as_bytes(),keys.temporal());
    ap.effect_complete(install.id,true).unwrap();
    assert!(!ap.is_authorized(STA));
    let authorization = ap.next_effect().unwrap(); assert!(matches!(authorization.operation,Operation::Authorize { peer: STA }));
    ap.effect_complete(authorization.id,true).unwrap(); assert!(ap.is_authorized(STA));
    assert_eq!(ap.receive_eapol(STA,&fourth[..n],Milliseconds(3),&mut random,&memory.clock()),Err(Error::Replay));
}
#[test]
fn wpa_tkip_authenticates_and_delivers_group_key_with_rc4() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,gtk,_) = ready(Protocol::Wpa,Cipher::Tkip,&mut random,&memory);
    let (keys,nonce) = begin(&mut ap,Protocol::Wpa,Cipher::Tkip,WPA,&mut random,&memory);
    let third = next_tx(&mut ap);
    let Operation::Transmit { frame, .. } = &third.operation else { unreachable!() };
    assert_eq!(&frame.as_bytes()[99..],WPA);
    let (fourth,n) = response(Protocol::Wpa,Cipher::Tkip,frame.as_bytes(),&keys,&nonce,&[],true);
    ap.effect_complete(third.id,true).unwrap();
    ap.receive_eapol(STA,&fourth[..n],Milliseconds(2),&mut random,&memory.clock()).unwrap();
    let install = ap.next_effect().unwrap();
    let Operation::Install { key, .. } = &install.operation else { panic!("pairwise installation") };
    assert_eq!(key.as_bytes(),keys.temporal_tkip()); ap.effect_complete(install.id,true).unwrap();
    let authorization = ap.next_effect().unwrap(); ap.effect_complete(authorization.id,true).unwrap();
    let group = next_tx(&mut ap);
    let Operation::Transmit { frame, encrypt, .. } = &group.operation else { unreachable!() };
    assert!(encrypt);
    let mut stream_key = [0;32]; stream_key[..16].copy_from_slice(&frame.as_bytes()[49..65]); stream_key[16..].copy_from_slice(keys.encryption());
    let mut plain = [0;32]; plain.copy_from_slice(&frame.as_bytes()[99..]); crypto::legacy::rc4_skip(&stream_key,256,&mut plain).unwrap();
    assert_eq!(plain,gtk);
    let (reply,n) = response(Protocol::Wpa,Cipher::Tkip,frame.as_bytes(),&keys,&nonce,&[],false);
    ap.effect_complete(group.id,true).unwrap(); ap.receive_eapol(STA,&reply[..n],Milliseconds(3),&mut random,&memory.clock()).unwrap();
    assert!(ap.is_authorized(STA)); assert!(ap.is_drained());
}
#[test]
fn lost_handshake_retries_are_bounded_and_disconnect() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,_,_) = ready(Protocol::Wpa2,Cipher::Ccmp,&mut random,&memory);
    ap.associated(STA,RSN,Milliseconds(0)).unwrap();
    let mut last_replay = 0;
    for attempt in 0..4 {
        let tx = next_tx(&mut ap);
        let Operation::Transmit { frame, .. } = &tx.operation else { unreachable!() };
        let replay = u64::from_be_bytes(frame.as_bytes()[9..17].try_into().unwrap()); assert!(replay > last_replay); last_replay = replay;
        ap.effect_complete(tx.id,true).unwrap(); ap.tick(Milliseconds((attempt + 1) * 1000),&mut random,&memory.clock()).unwrap();
    }
    let revoke = ap.next_effect().unwrap(); assert!(matches!(revoke.operation,Operation::Unauthorize { peer: STA })); ap.effect_complete(revoke.id,true).unwrap();
    let remove = ap.next_effect().unwrap(); assert!(matches!(remove.operation,Operation::Remove { peer: Some(STA), .. }));
    ap.effect_complete(remove.id,true).unwrap(); let deauth = ap.next_effect().unwrap();
    assert!(matches!(deauth.operation,Operation::Deauthenticate { peer: STA, reason: 15 }));
    assert!(!ap.is_authorized(STA));
}
#[test]
fn bad_mic_never_produces_key_install_or_authorization() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,_,_) = ready(Protocol::Wpa2,Cipher::Ccmp,&mut random,&memory);
    ap.associated(STA,RSN,Milliseconds(0)).unwrap(); let tx = next_tx(&mut ap);
    let Operation::Transmit { frame, .. } = &tx.operation else { unreachable!() };
    let nonce = [0x39;32]; let anonce = frame.as_bytes()[17..49].try_into().unwrap();
    let keys = crypto::derive_pairwise_keys(&crypto::derive_master_key(b"password",b"IEEE"),&AP.0,&STA.0,anonce,&nonce);
    let (mut reply,n) = response(Protocol::Wpa2,Cipher::Ccmp,frame.as_bytes(),&keys,&nonce,RSN,true); reply[81] ^= 1;
    ap.effect_complete(tx.id,true).unwrap();
    assert_eq!(ap.receive_eapol(STA,&reply[..n],Milliseconds(1),&mut random,&memory.clock()),Err(Error::InvalidMic));
    assert!(ap.next_effect().is_none()); assert!(!ap.is_authorized(STA));
}
#[test]
fn cancellation_invalidates_outstanding_peer_completion_and_removes_keys() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,_,_) = ready(Protocol::Wpa2,Cipher::Ccmp,&mut random,&memory);
    ap.associated(STA,RSN,Milliseconds(0)).unwrap(); let tx = next_tx(&mut ap);
    ap.cancel().unwrap(); assert!(!ap.is_ready()); assert!(!ap.is_drained());
    assert_eq!(ap.effect_complete(tx.id,true),Err(Error::StaleCompletion));
    let mut removed_group = [false;2];
    while let Some(effect) = ap.next_effect() {
        match effect.operation { Operation::Remove { peer: None,index } => removed_group[(index - 1) as usize] = true,
            Operation::Remove { .. } | Operation::Unauthorize { .. } | Operation::Deauthenticate { .. } => {}, _ => panic!("canceled operation escaped") }
        ap.effect_complete(effect.id,true).unwrap();
    }
    assert_eq!(removed_group,[true,true]); assert!(ap.is_drained());
}
#[test]
fn incompatible_enterprise_and_truncated_association_are_rejected() {
    assert_eq!(validate_association(Protocol::Wpa2,Cipher::Ccmp,&RSN[..21]),Err(Error::InvalidElement));
    let mut enterprise = [0;22]; enterprise.copy_from_slice(RSN); enterprise[19] = 1;
    assert_eq!(validate_association(Protocol::Wpa2,Cipher::Ccmp,&enterprise),Err(Error::UnsupportedSecurity));
    assert_eq!(validate_association(Protocol::Wpa2,Cipher::Tkip,RSN),Err(Error::UnsupportedSecurity));
}

#[test]
fn failed_initial_group_install_never_starts_the_bss() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let mut ap = auth(Protocol::Wpa2,Cipher::Ccmp); ap.initialize(&mut random,&memory.clock()).unwrap();
    let install = ap.next_effect().unwrap();
    assert_eq!(ap.effect_complete(install.id,false),Err(Error::PhysicalFailure));
    assert!(!ap.is_ready());
    assert_eq!(ap.associated(STA,RSN,Milliseconds(0)),Err(Error::NotReady));
    assert_eq!(ap.effect_complete(install.id,true),Err(Error::StaleCompletion));
}

#[test]
fn thirty_two_byte_credential_is_a_passphrase_not_a_raw_master_key() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let password = b"0123456789abcdefghijklmnopqrstuv";
    let mut ap = Authenticator::new(AP,Protocol::Wpa2,Cipher::Ccmp,&Ssid::parse(b"IEEE").unwrap(),password).unwrap();
    ap.initialize(&mut random,&memory.clock()).unwrap(); let install = ap.next_effect().unwrap(); ap.effect_complete(install.id,true).unwrap();
    ap.associated(STA,RSN,Milliseconds(0)).unwrap(); let first = next_tx(&mut ap);
    let Operation::Transmit { frame, .. } = &first.operation else { unreachable!() };
    let nonce = [0x39;32]; let anonce = frame.as_bytes()[17..49].try_into().unwrap();
    let keys = crypto::derive_pairwise_keys(&crypto::derive_master_key(password,b"IEEE"),&AP.0,&STA.0,anonce,&nonce);
    let (reply,n) = response(Protocol::Wpa2,Cipher::Ccmp,frame.as_bytes(),&keys,&nonce,RSN,true);
    ap.effect_complete(first.id,true).unwrap();
    ap.receive_eapol(STA,&reply[..n],Milliseconds(1),&mut random,&memory.clock()).unwrap();
    let third = next_tx(&mut ap); let Operation::Transmit { frame, .. } = third.operation else { unreachable!() };
    assert_eq!(u16::from_be_bytes([frame.as_bytes()[5],frame.as_bytes()[6]]) & 0x13f8,0x13c8);
}

#[test]
fn group_rotation_waits_for_peer_acknowledgment_before_driver_activation() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,old_gtk,_) = ready(Protocol::Wpa2,Cipher::Ccmp,&mut random,&memory);
    let (keys,nonce) = begin(&mut ap,Protocol::Wpa2,Cipher::Ccmp,RSN,&mut random,&memory);
    let third = next_tx(&mut ap); let Operation::Transmit { frame, .. } = &third.operation else { unreachable!() };
    let (fourth,n) = response(Protocol::Wpa2,Cipher::Ccmp,frame.as_bytes(),&keys,&nonce,&[],true);
    ap.effect_complete(third.id,true).unwrap(); ap.receive_eapol(STA,&fourth[..n],Milliseconds(2),&mut random,&memory.clock()).unwrap();
    let install = ap.next_effect().unwrap(); ap.effect_complete(install.id,true).unwrap();
    let authorize = ap.next_effect().unwrap(); ap.effect_complete(authorize.id,true).unwrap();
    ap.rekey_group(&mut random,&memory.clock()).unwrap();
    let group = next_tx(&mut ap); let Operation::Transmit { frame, .. } = &group.operation else { unreachable!() };
    let mut plain = [0;64]; let length = crypto::unwrap_key_data(keys.encryption(),&frame.as_bytes()[99..],&mut plain).unwrap();
    assert!(length >= 24); assert_eq!(plain[6],2); assert_ne!(&plain[8..24],&old_gtk[..16]);
    let (reply,n) = response(Protocol::Wpa2,Cipher::Ccmp,frame.as_bytes(),&keys,&nonce,&[],false);
    ap.effect_complete(group.id,true).unwrap(); assert!(ap.next_effect().is_none());
    ap.receive_eapol(STA,&reply[..n],Milliseconds(3),&mut random,&memory.clock()).unwrap();
    let activation = ap.next_effect().unwrap();
    let Operation::Install { peer: None,index: 2,key,.. } = &activation.operation else { panic!("new GTK activation") };
    assert_eq!(key.as_bytes(),&plain[8..24]); ap.effect_complete(activation.id,true).unwrap(); assert!(ap.is_authorized(STA));
}

#[test]
fn stop_preserves_cleanup_already_queued_for_a_disconnected_peer() {
    let memory = Memory::new(); let mut random = RandomPool::new();
    let (mut ap,_,_) = ready(Protocol::Wpa2,Cipher::Ccmp,&mut random,&memory);
    ap.associated(STA,RSN,Milliseconds(0)).unwrap();
    ap.disconnected(STA).unwrap(); ap.cancel().unwrap();
    let mut revoked = false; let mut removed = false;
    while let Some(effect) = ap.next_effect() {
        match effect.operation {
            Operation::Unauthorize { peer: STA } => revoked = true,
            Operation::Remove { peer: Some(STA), .. } => removed = true,
            Operation::Remove { peer: None, .. } => {},
            _ => panic!("stale authentication escaped teardown"),
        }
        ap.effect_complete(effect.id,true).unwrap();
    }
    assert!(revoked && removed); assert!(ap.is_drained());
}
