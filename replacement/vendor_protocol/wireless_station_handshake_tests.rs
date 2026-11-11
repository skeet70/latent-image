use super::{EapolFrame, HandshakeError, HandshakePhase, KeyMaterial, RsnSupplicant};
use super::super::crypto::{derive_pairwise_keys, hmac_sha1, MasterKey, PairwiseKeys};
use super::super::crypto::legacy::{hmac_md5, rc4_skip};
use super::super::profile::{Cipher, InformationElements, PersonalProtocol, SelectedAccessPoint};
use super::super::MacAddress;

const LOCAL: MacAddress = MacAddress([2,0,0,0,0,1]);
const PEER: MacAddress = MacAddress([2,0,0,0,0,2]);
fn elements(protocol: PersonalProtocol, group: Cipher) -> InformationElements {
    let group = match group { Cipher::Wep40=>1, Cipher::Tkip=>2, Cipher::Ccmp=>4, Cipher::Wep104=>5, _=>0 };
    let bytes = match protocol {
        PersonalProtocol::Wpa => vec![221,22,0,80,242,1,1,0,0,80,242,group,1,0,0,80,242,2,1,0,0,80,242,2],
        PersonalProtocol::Rsn => vec![48,20,1,0,0,15,172,group,1,0,0,15,172,2,1,0,0,15,172,2,0,0],
        PersonalProtocol::None => vec![],
    };
    InformationElements::parse(&bytes).unwrap()
}
fn frame(protocol: PersonalProtocol, info: u16, replay: u8, key_length: u16, data: &[u8], keys: Option<&PairwiseKeys>) -> Vec<u8> {
    let mut bytes = vec![0;99+data.len()]; bytes[0]=1; bytes[1]=3;
    bytes[2..4].copy_from_slice(&((95+data.len()) as u16).to_be_bytes());
    bytes[4]=if protocol==PersonalProtocol::Wpa {254} else {2}; bytes[5..7].copy_from_slice(&info.to_be_bytes());
    bytes[7..9].copy_from_slice(&key_length.to_be_bytes()); bytes[16]=replay; bytes[17..49].fill(0x22);
    bytes[49..65].fill(0x33); bytes[65..71].copy_from_slice(&[1,2,3,4,5,6]);
    bytes[97..99].copy_from_slice(&(data.len() as u16).to_be_bytes()); bytes[99..].copy_from_slice(data);
    if let Some(keys)=keys { sign(&mut bytes,keys); } bytes
}
fn sign(bytes: &mut [u8], keys: &PairwiseKeys) {
    bytes[81..97].fill(0);
    let digest = if bytes[6]&7==1 {hmac_md5(keys.confirmation(),&[bytes])} else {hmac_sha1(keys.confirmation(),&[bytes])[..16].try_into().unwrap()};
    bytes[81..97].copy_from_slice(&digest);
}
fn decryptable(data: &[u8], keys: &PairwiseKeys) -> Vec<u8> {
    let mut key=[0x33;32]; key[16..].copy_from_slice(keys.encryption());
    let mut encrypted=data.to_vec(); rc4_skip(&key,256,&mut encrypted).unwrap(); encrypted
}
fn begin(protocol: PersonalProtocol, group: Cipher) -> (RsnSupplicant,PairwiseKeys,InformationElements) {
    let advertised=elements(protocol,group);
    let ap=SelectedAccessPoint { address:PEER,protocol,pairwise:Cipher::Tkip,group,advertised_elements:advertised.clone() };
    let master=MasterKey::from_bytes([0x11;32]);
    let keys=derive_pairwise_keys(&master,&LOCAL.0,&PEER.0,&[0x44;32],&[0x22;32]);
    let mut station=RsnSupplicant::new(LOCAL,ap,master,[0x44;32]).unwrap();
    let transaction=station.receive(PEER,&frame(protocol,0x89,1,32,&[],None)).unwrap();
    let response=transaction.reply.as_bytes(); assert_eq!(&response[17..49],&[0x44;32]);
    assert_eq!(&response[99..],advertised.as_bytes()); assert!(transaction.pairwise.is_none()); assert!(!transaction.completes);
    (station,keys,advertised)
}
fn verify_reply(reply: &EapolFrame, keys: &PairwiseKeys) {
    let mut bytes=reply.as_bytes().to_vec(); let mic=bytes[81..97].to_vec(); sign(&mut bytes,keys); assert_eq!(mic,bytes[81..97]);
}
#[test]
fn wpa_tkip_pairwise_and_all_group_ciphers() {
    for cipher in [Cipher::Wep40,Cipher::Wep104,Cipher::Tkip,Cipher::Ccmp] {
        let protocol=PersonalProtocol::Wpa; let (mut station,keys,ie)=begin(protocol,cipher);
        let third=frame(protocol,0x1c9,2,32,ie.as_bytes(),Some(&keys));
        let transaction=station.receive(PEER,&third).unwrap(); verify_reply(&transaction.reply,&keys);
        assert!(matches!(transaction.pairwise.unwrap().material,KeyMaterial::Tkip(_))); assert!(!transaction.completes);
        let length=match cipher {Cipher::Wep40=>5,Cipher::Wep104=>13,Cipher::Tkip=>32,_=>16};
        let key: Vec<u8>=(0..length).collect(); let encrypted=decryptable(&key,&keys);
        let group=frame(protocol,0x3a1,3,length as u16,&encrypted,Some(&keys));
        let transaction=station.receive(PEER,&group).unwrap(); verify_reply(&transaction.reply,&keys);
        assert!(transaction.completes); assert!(transaction.pairwise.is_none());
        let installed=transaction.group.unwrap(); assert_eq!(installed.slot.index.get(),2); assert!(!installed.transmit);
        match installed.material {
            KeyMaterial::Wep(value)=>{assert_eq!(value.as_bytes(),key);assert_eq!(installed.receive_sequence,[0;6]);},
            KeyMaterial::Ccmp(value)=>assert_eq!(value.as_slice(),key),
            KeyMaterial::Tkip(value)=>assert_eq!(value,[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,24,25,26,27,28,29,30,31,16,17,18,19,20,21,22,23]),
        }
        assert_eq!(station.phase(),HandshakePhase::Complete);
        assert!(matches!(station.receive(PEER,&group),Err(HandshakeError::Replay)));
    }
}
#[test]
fn rsn_tkip_rc4_kde_is_authenticated_and_installed() {
    let protocol=PersonalProtocol::Rsn; let (mut station,keys,ie)=begin(protocol,Cipher::Tkip);
    let mut data=ie.as_bytes().to_vec(); data.extend_from_slice(&[221,38,0,15,172,1,1,0]); data.extend(0u8..32);
    let encrypted=decryptable(&data,&keys); let third=frame(protocol,0x13c9,2,32,&encrypted,Some(&keys));
    let mut bad=third.clone(); bad[110]^=1;
    assert!(matches!(station.receive(PEER,&bad),Err(HandshakeError::Mic)));
    let transaction=station.receive(PEER,&third).unwrap(); assert!(transaction.completes);
    assert_eq!(transaction.group.unwrap().slot.index.get(),1); assert_eq!(transaction.pairwise.unwrap().receive_sequence,[0;6]);
    assert!(matches!(station.receive(PEER,&third),Err(HandshakeError::Replay)));
}
#[test]
fn authenticated_bad_nonce_and_ie_do_not_install() {
    let protocol=PersonalProtocol::Wpa; let (mut station,keys,ie)=begin(protocol,Cipher::Tkip);
    let mut third=frame(protocol,0x1c9,2,32,ie.as_bytes(),Some(&keys)); third[17]^=1; sign(&mut third,&keys);
    let error=station.receive(PEER,&third).unwrap_err(); assert_eq!(error,HandshakeError::Nonce); assert!(error.disconnects());
    let mut wrong=ie.as_bytes().to_vec(); wrong[11]=4;
    let error=station.receive(PEER,&frame(protocol,0x1c9,2,32,&wrong,Some(&keys))).unwrap_err();
    assert_eq!(error,HandshakeError::SecurityElements); assert!(error.disconnects());
    assert!(station.receive(PEER,&frame(protocol,0x1c9,2,32,ie.as_bytes(),Some(&keys))).is_ok());
}
#[test]
fn descriptor_and_peer_mismatch_do_not_advance_handshake() {
    let protocol=PersonalProtocol::Wpa; let (mut station,keys,ie)=begin(protocol,Cipher::Tkip);
    let mut third=frame(protocol,0x1c9,2,32,ie.as_bytes(),Some(&keys)); third[4]=2;
    let error=station.receive(PEER,&third).unwrap_err(); assert_eq!(error,HandshakeError::Descriptor); assert!(!error.disconnects());
    assert!(matches!(station.receive(LOCAL,&third),Err(HandshakeError::Peer)));
    third[4]=254; third[6]=(third[6]&!7)|3;
    assert!(matches!(station.receive(PEER,&third),Err(HandshakeError::Descriptor)));
}

fn wrap(key: &[u8;16], data: &[u8]) -> Vec<u8> {
    let aes=super::super::crypto::Aes128::new(key);
    let count=data.len()/8; let mut a=[0xa6;8]; let mut result=data.to_vec();
    for j in 0..6 {
        for i in 0..count {
            let mut block=[0;16]; block[..8].copy_from_slice(&a); block[8..].copy_from_slice(&result[i*8..i*8+8]);
            aes.encrypt(&mut block);
            let counter=(count*j+i+1) as u64;
            a.copy_from_slice(&(u64::from_be_bytes(block[..8].try_into().unwrap())^counter).to_be_bytes());
            result[i*8..i*8+8].copy_from_slice(&block[8..]);
        }
    }
    let mut output=a.to_vec(); output.extend_from_slice(&result); output
}
#[test]
fn rsn_ccmp_aes_wrapped_group_and_mic_rejection() {
    let protocol=PersonalProtocol::Rsn;
    let ie=InformationElements::parse(&[48,20,1,0,0,15,172,4,1,0,0,15,172,4,1,0,0,15,172,2,0,0]).unwrap();
    let ap=SelectedAccessPoint {address:PEER,protocol,pairwise:Cipher::Ccmp,group:Cipher::Ccmp,advertised_elements:ie.clone()};
    let master=MasterKey::from_bytes([0x11;32]);
    let keys=derive_pairwise_keys(&master,&LOCAL.0,&PEER.0,&[0x44;32],&[0x22;32]);
    let mut station=RsnSupplicant::new(LOCAL,ap,master,[0x44;32]).unwrap();
    let first=station.receive(PEER,&frame(protocol,0x8a,1,16,&[],None)).unwrap(); verify_reply(&first.reply,&keys);
    let mut data=ie.as_bytes().to_vec(); data.extend_from_slice(&[221,22,0,15,172,1,3,0]); data.extend_from_slice(&[0x55;16]); data.extend_from_slice(&[221,0]);
    let encrypted=wrap(keys.encryption(),&data);
    let third=frame(protocol,0x13ca,2,16,&encrypted,Some(&keys)); let mut invalid=third.clone(); invalid[81]^=1;
    assert!(matches!(station.receive(PEER,&invalid),Err(HandshakeError::Mic)));
    let mut bad_wrap=third.clone(); bad_wrap[99]^=1; sign(&mut bad_wrap,&keys);
    let error=station.receive(PEER,&bad_wrap).unwrap_err(); assert_eq!(error,HandshakeError::Decryption); assert!(!error.disconnects());
    let transaction=station.receive(PEER,&third).unwrap(); verify_reply(&transaction.reply,&keys); assert!(transaction.completes);
    let group=transaction.group.unwrap(); assert_eq!(group.material,KeyMaterial::Ccmp([0x55;16])); assert_eq!(group.slot.index.get(),3);
    assert_eq!(group.receive_sequence,[1,2,3,4,5,6]);
    let next=frame(protocol,0x8a,3,16,&[],None);
    assert!(matches!(station.receive(PEER,&next),Err(HandshakeError::NonceRequired)));
    assert_eq!(station.begin_rekey([0x44;32]),Err(HandshakeError::Nonce));
    station.begin_rekey([0x66;32]).unwrap();
    let reply=station.receive(PEER,&next).unwrap();
    assert_eq!(&reply.reply.as_bytes()[17..49],&[0x66;32]); assert!(reply.pairwise.is_none());
}
