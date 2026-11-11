use super::{CountermeasureDriver, SecurityAction, SecurityError, TkipCountermeasures};
use super::super::crypto::{derive_pairwise_keys, MasterKey};
use super::super::crypto::legacy::hmac_md5;
use super::super::crypto::random::{ClockScratch, RtosClock};
use super::super::handshake::{HandshakeError, RsnSupplicant};
use super::super::profile::{Cipher, InformationElements, PersonalProtocol, SelectedAccessPoint};
use xm1_rtos_own_first::{Cpsr, DebugIdentity, Domain, GuestAddress, Machine};
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_station_driver::association::{MicFailure, MicKey};
use core::cell::{Cell, RefCell};
use std::collections::BTreeMap;

struct Memory { words: RefCell<BTreeMap<u32,u32>>, cpsr: Cell<Cpsr> }
impl Memory {
    fn new(time: u32) -> Self { let memory=Self {words:RefCell::new(BTreeMap::new()),cpsr:Cell::new(Cpsr(0x13))}; memory.time(time); memory }
    fn time(&self,time:u32) {self.store_word(Domain::A.words().timer_base,time);}
    fn clock(&self) -> RtosClock<'_,Self> {RtosClock::new(self,Domain::A,ClockScratch::new(GuestAddress(0x2000)).unwrap())}
}
impl Machine for Memory {
    fn load_word(&self,at:GuestAddress)->u32 {self.words.borrow().get(&at.0).copied().unwrap_or(0)}
    fn load_byte(&self,at:GuestAddress)->u8 {(self.load_word(GuestAddress(at.0&!3))>>((at.0&3)*8)) as u8}
    fn store_word(&self,at:GuestAddress,value:u32) {self.words.borrow_mut().insert(at.0,value);}
    fn cpsr(&self)->Cpsr {self.cpsr.get()}
    fn write_cpsr_control(&self,value:Cpsr) {self.cpsr.set(value);}
    fn debug_identity(&self)->DebugIdentity {DebugIdentity::PRIMARY}
}
const LOCAL:MacAddress=MacAddress([2,0,0,0,0,1]);
const PEER:MacAddress=MacAddress([2,0,0,0,0,2]);
fn event(key:MicKey)->MicFailure {MicFailure {bssid:PEER,key}}
fn station() -> RsnSupplicant {
    let ie=InformationElements::parse(&[221,22,0,80,242,1,1,0,0,80,242,2,1,0,0,80,242,2,1,0,0,80,242,2]).unwrap();
    RsnSupplicant::new(LOCAL,SelectedAccessPoint {address:PEER,protocol:PersonalProtocol::Wpa,pairwise:Cipher::Tkip,group:Cipher::Tkip,advertised_elements:ie},MasterKey::from_bytes([0x11;32]),[0x44;32]).unwrap()
}
#[test]
fn two_failures_report_then_drain_disconnect_and_resume() {
    let memory=Memory::new(1900); let clock=memory.clock(); let mut engine=TkipCountermeasures::new(); let mut station=station();
    assert_eq!(engine.driver_capability(),CountermeasureDriver::Unsupported);
    let SecurityAction::Report(first)=engine.failure(event(MicKey::Pairwise),&clock,&mut station).unwrap() else {panic!("first report")};
    assert_eq!(&first.as_bytes()[5..7],&[0x0c,0x09]); assert_eq!(&first.as_bytes()[9..17],&[0;8]); assert!(!engine.active());
    memory.time(61_999);
    let SecurityAction::ReportAndStart(second)=engine.failure(event(MicKey::Group),&clock,&mut station).unwrap() else {panic!("countermeasure report")};
    assert_eq!(&second.as_bytes()[5..7],&[0x0c,0x01]); assert_eq!(second.as_bytes()[16],1); assert!(engine.active());
    memory.time(62_008); assert!(matches!(engine.poll(&clock).unwrap(),SecurityAction::None));
    memory.time(62_009); assert!(matches!(engine.poll(&clock).unwrap(),SecurityAction::Disconnect));
    assert!(matches!(engine.poll(&clock).unwrap(),SecurityAction::None));
    memory.time(122_008); assert!(matches!(engine.poll(&clock).unwrap(),SecurityAction::None));
    memory.time(122_009); assert!(matches!(engine.poll(&clock).unwrap(),SecurityAction::Resume)); assert!(!engine.active());
}
#[test]
fn expired_window_starts_new_first_failure_and_reset_cancels_blocking() {
    let memory=Memory::new(1900); let clock=memory.clock(); let mut engine=TkipCountermeasures::new(); let mut station=station();
    assert!(matches!(engine.failure(event(MicKey::Pairwise),&clock,&mut station).unwrap(),SecurityAction::Report(_)));
    memory.time(62_000); assert!(matches!(engine.failure(event(MicKey::Group),&clock,&mut station).unwrap(),SecurityAction::Report(_)));
    memory.time(62_001); assert!(matches!(engine.failure(event(MicKey::Group),&clock,&mut station).unwrap(),SecurityAction::ReportAndStart(_)));
    assert!(matches!(engine.failure(event(MicKey::Group),&clock,&mut station).unwrap(),SecurityAction::None));
    engine.reset(); assert!(!engine.active()); memory.time(122_001); assert!(matches!(engine.poll(&clock).unwrap(),SecurityAction::None));
    assert!(matches!(engine.failure(event(MicKey::Group),&clock,&mut station).unwrap(),SecurityAction::Report(_)));
}
#[test]
fn bad_peer_and_failed_actual_clock_do_not_count_as_failures() {
    let memory=Memory::new(1000); let clock=memory.clock(); let mut engine=TkipCountermeasures::new(); let mut station=station();
    assert!(matches!(engine.failure(MicFailure {bssid:LOCAL,key:MicKey::Pairwise},&clock,&mut station),Err(SecurityError::Handshake(HandshakeError::Peer))));
    memory.store_word(Domain::A.words().handler_context,1);
    assert!(matches!(engine.failure(event(MicKey::Pairwise),&clock,&mut station),Err(SecurityError::Clock(_))));
    memory.store_word(Domain::A.words().handler_context,0);
    assert!(matches!(engine.failure(event(MicKey::Pairwise),&clock,&mut station).unwrap(),SecurityAction::Report(_)));
}
#[test]
fn confirmed_tkip_request_has_authenticated_independent_replay_counter() {
    let mut station=station();
    let mut first=vec![0;99]; first[..5].copy_from_slice(&[1,3,0,95,254]); first[5..7].copy_from_slice(&0x89u16.to_be_bytes()); first[7..9].copy_from_slice(&32u16.to_be_bytes()); first[16]=1; first[17..49].fill(0x22);
    station.receive(PEER,&first).unwrap();
    let ie=station.association_elements().as_bytes(); let mut third=vec![0;99+ie.len()]; let payload_length=(95+ie.len()) as u16; third[..99].copy_from_slice(&first); third[2..4].copy_from_slice(&payload_length.to_be_bytes()); third[5..7].copy_from_slice(&0x1c9u16.to_be_bytes()); third[16]=2; third[97..99].copy_from_slice(&(ie.len() as u16).to_be_bytes()); third[99..].copy_from_slice(ie);
    let keys=derive_pairwise_keys(&MasterKey::from_bytes([0x11;32]),&LOCAL.0,&PEER.0,&[0x44;32],&[0x22;32]); let mic=hmac_md5(keys.confirmation(),&[&third]); third[81..97].copy_from_slice(&mic); station.receive(PEER,&third).unwrap();
    for counter in 0..2 {
        let request=station.michael_failure_request(event(MicKey::Pairwise)).unwrap(); let bytes=request.as_bytes();
        assert_eq!(&bytes[5..7],&[0x0d,0x09]); assert_eq!(bytes[16],counter); assert_eq!(&bytes[7..9],&[0,0]);
        let expected=hmac_md5(keys.confirmation(),&[&bytes[..81],&[0;16],&bytes[97..]]); assert_eq!(&bytes[81..97],&expected);
    }
}
