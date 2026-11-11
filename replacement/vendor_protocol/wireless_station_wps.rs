//! X-M1 enrollee transitions: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3,
//! wps_enrollee.c and wps_enrollee_rest.c.

use super::{MacAddress, Ssid, StationProfile, WpsPin};
use super::crypto::wps::{DhPrivate,DhPublic,Nonce,SecretNonce,Authenticator,Commitment,EnrolleeMac,Iv,WpsKeys,PasswordHalf};
#[path="wireless_station_wps_wire.rs"]
mod wire;
pub use wire::WpsFrame;
use wire::{Attribute as A,Attributes,Message as M};
#[cfg(test)]
#[path="wireless_station_wps_tests.rs"]
mod exchange_tests;
#[path="wireless_station_wps_registrar.rs"]
pub mod registrar;

#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum WpsMethod { Pbc, Pin(WpsPin), GeneratePin }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub struct WpsConfiguration { pub method:WpsMethod, pub bssid:Option<MacAddress> }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum WpsError { State,Peer,Capacity,Truncated,Duplicate,MissingAttribute,AttributeLength,Message,Authentication,Crypto,Credential,Timeout,Registrar,Overlap }
#[derive(Clone,Debug,Eq,PartialEq)]
pub enum WpsOutcome { Credentials(StationProfile),Cancelled,Failed(WpsError) }
#[derive(Debug)]
pub enum WpsEffect { Entropy,Scan,Associate{peer:MacAddress},Transmit{peer:MacAddress,frame:WpsFrame},ArmTimer{milliseconds:u32},CancelTimer,Disconnect }
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum WpsState { Idle,Entropy,Scanning,Start,M2,M4,M6,M8,Finished,Failed }

#[derive(Clone)]
pub struct WpsNetworkKey { bytes:[u8;64], length:u8 }
impl WpsNetworkKey {pub fn as_bytes(&self)->&[u8]{&self.bytes[..self.length as usize]}}
impl Drop for WpsNetworkKey {fn drop(&mut self){self.bytes.fill(0);}}
impl core::fmt::Debug for WpsNetworkKey {fn fmt(&self,f:&mut core::fmt::Formatter<'_>)->core::fmt::Result{f.write_str("WpsNetworkKey([redacted])")}}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
#[repr(u8)]
pub enum WpsSecurity {Open=0,Wep=1,Wpa=2,Wpa2=3}
#[derive(Clone,Debug)]
pub struct WpsCredential {pub profile:StationProfile, key:WpsNetworkKey, security:WpsSecurity, index:super::KeyIndex}
impl WpsCredential {
    pub fn key(&self)->&[u8]{self.key.as_bytes()}
    pub const fn security(&self)->u8{self.security as u8}
    pub const fn ssid(&self)->Ssid{self.profile.ssid}
    pub const fn key_index(&self)->u8{self.index.get()}
}

pub struct WpsEnrollee {
    local:MacAddress, configuration:WpsConfiguration, state:WpsState, peer:Option<MacAddress>,
    effects:[Option<WpsEffect>;8], queued:usize, outcome:Option<WpsOutcome>,
    credential:Option<WpsCredential>,
    scan_runs:u8,
    entropy:[u8;276], entropy_used:usize, private:Option<DhPrivate>, public:Option<DhPublic>, registrar:Option<DhPublic>,
    nonce:Nonce, registrar_nonce:Nonce, secret1:SecretNonce, secret2:SecretNonce, iv:Iv, second_iv:Iv,
    keys:Option<WpsKeys>, hash1:Commitment,hash2:Commitment,
    last:WpsFrame, incoming:WpsFrame, incoming_total:Option<usize>, incoming_opcode:u8,
    outgoing:WpsFrame, outgoing_used:usize,outgoing_opcode:u8, last_response:Option<WpsFrame>, last_id:Option<u8>,
}
impl WpsEnrollee {
    pub fn new(local:MacAddress,configuration:WpsConfiguration)->Result<Self,WpsError> {
        Ok(Self{local,configuration,state:WpsState::Idle,peer:None,effects:core::array::from_fn(|_|None),queued:0,outcome:None,credential:None,scan_runs:1,entropy:[0;276],entropy_used:0,private:None,public:None,registrar:None,nonce:Nonce([0;16]),registrar_nonce:Nonce([0;16]),secret1:SecretNonce::from_bytes([0;16]),secret2:SecretNonce::from_bytes([0;16]),iv:Iv([0;16]),second_iv:Iv([0;16]),keys:None,hash1:Commitment([0;32]),hash2:Commitment([0;32]),last:WpsFrame::new(),incoming:WpsFrame::new(),incoming_total:None,incoming_opcode:0,outgoing:WpsFrame::new(),outgoing_used:0,outgoing_opcode:0,last_response:None,last_id:None})
    }
    pub const fn state(&self)->WpsState { self.state }
    pub const fn configuration(&self)->WpsConfiguration { self.configuration }
    pub fn pin(&self)->Option<WpsPin> { match self.configuration.method { WpsMethod::Pin(pin)=>Some(pin),_=>None } }
    pub fn credential(&self)->Option<&WpsCredential>{self.credential.as_ref()}
    pub fn retry_scan(&mut self)->Result<(),WpsError>{if self.state!=WpsState::Scanning{return Err(WpsError::State);}self.scan_runs=self.scan_runs.saturating_add(1);self.push(WpsEffect::Scan)}
    pub fn scan_eligible(&self,peer:MacAddress,elements:&[u8])->Result<bool,WpsError>{
        if self.configuration.bssid.is_some_and(|expected|expected!=peer){return Ok(false);}
        let mut rest=elements;let mut combined=WpsFrame::new();
        while !rest.is_empty(){if rest.len()<2{return Err(WpsError::Truncated);}let size=rest[1] as usize;if rest.len()<size+2{return Err(WpsError::Truncated);}let value=&rest[2..2+size];if rest[0]==221&&value.starts_with(&[0,0x50,0xf2,4]){combined.append(&value[4..])?;}rest=&rest[2+size..];}
        if combined.as_bytes().is_empty(){return Ok(false);}let attrs=Attributes::parse(combined.as_bytes())?;
        let selected=attrs.optional(A::SelectedRegistrar)?.is_some_and(|value|matches!(value,[value] if *value!=0));
        let password=attrs.optional(A::PasswordId)?.map(|value|value.try_into().map(u16::from_be_bytes).map_err(|_|WpsError::AttributeLength)).transpose()?;
        if self.configuration.method==WpsMethod::Pbc{return Ok(selected&&password==Some(4));}
        if self.scan_runs>=3{return Ok(true);}
        let mut version2=false;let mut authorized=false;
        for (tag,value) in attrs.all(){if tag!=0x1049||!value.starts_with(&[0,0x37,0x2a]){continue;}let mut extensions=&value[3..];while !extensions.is_empty(){if extensions.len()<2||extensions.len()<2+extensions[1] as usize{return Err(WpsError::Truncated);}let kind=extensions[0];let length=extensions[1] as usize;let data=&extensions[2..2+length];if kind==0{version2=true;}if kind==1{if length%6!=0{return Err(WpsError::AttributeLength);}authorized|=data.chunks_exact(6).any(|mac|mac==self.local.0||mac==[0xff;6]);}extensions=&extensions[2+length..];}}
        Ok(if version2{authorized}else{selected&&password!=Some(4)})
    }
    pub fn association_elements(&self)->&[u8] { &[221,14,0,0x50,0xf2,4,0x10,0x4a,0,1,0x10,0x10,0x3a,0,1,1] }
    fn push(&mut self,effect:WpsEffect)->Result<(),WpsError> { if self.queued==self.effects.len(){return Err(WpsError::Capacity);} self.effects[self.queued]=Some(effect);self.queued+=1;Ok(()) }
    pub fn poll(&mut self)->Option<WpsEffect> { if self.queued==0{return None;} let result=self.effects[0].take();for i in 1..self.queued {self.effects[i-1]=self.effects[i].take();} self.queued-=1;result }
    pub fn take_outcome(&mut self)->Option<WpsOutcome> { self.outcome.take() }
    pub fn start(&mut self)->Result<(),WpsError> { if self.state!=WpsState::Idle{return Err(WpsError::State);} self.state=WpsState::Entropy;self.push(WpsEffect::Entropy) }
    pub fn supply_entropy(&mut self,mut entropy:[u8;32])->Result<(),WpsError> {
        if self.state!=WpsState::Entropy {entropy.fill(0);return Err(WpsError::State);}
        let count=(self.entropy.len()-self.entropy_used).min(32);self.entropy[self.entropy_used..self.entropy_used+count].copy_from_slice(&entropy[..count]);entropy.fill(0);self.entropy_used+=count;
        if self.entropy_used<self.entropy.len(){return self.push(WpsEffect::Entropy);}
        let private=match DhPrivate::from_bytes(self.entropy[..192].try_into().map_err(|_|WpsError::Crypto)?) {Ok(private)=>private,Err(_)=>{self.entropy.fill(0);self.entropy_used=0;return self.push(WpsEffect::Entropy);}};
        self.public=Some(private.public_key());self.private=Some(private);
        self.nonce=Nonce(self.entropy[192..208].try_into().map_err(|_|WpsError::Crypto)?);self.secret1=SecretNonce::from_bytes(self.entropy[208..224].try_into().map_err(|_|WpsError::Crypto)?);self.secret2=SecretNonce::from_bytes(self.entropy[224..240].try_into().map_err(|_|WpsError::Crypto)?);self.iv=Iv(self.entropy[240..256].try_into().map_err(|_|WpsError::Crypto)?);self.second_iv=Iv(self.entropy[256..272].try_into().map_err(|_|WpsError::Crypto)?);
        if self.configuration.method==WpsMethod::GeneratePin {self.configuration.method=WpsMethod::Pin(WpsPin::generate(self.entropy[272..276].try_into().map_err(|_|WpsError::Crypto)?));}
        self.entropy.fill(0);self.state=WpsState::Scanning;self.push(WpsEffect::ArmTimer{milliseconds:120_000})?;self.push(WpsEffect::Scan)
    }
    pub fn select(&mut self,peer:MacAddress)->Result<(),WpsError> { if self.state!=WpsState::Scanning{return Err(WpsError::State);} if self.configuration.bssid.is_some_and(|expected|expected!=peer){return Err(WpsError::Peer);} self.push(WpsEffect::Associate{peer}) }
    pub fn associate(&mut self,peer:MacAddress)->Result<(),WpsError> { if self.state!=WpsState::Scanning{return Err(WpsError::State);} if self.configuration.bssid.is_some_and(|expected|expected!=peer){return Err(WpsError::Peer);} self.peer=Some(peer);self.state=WpsState::Start;let mut frame=WpsFrame::new();frame.append(&[1,1,0,0])?;self.transmit(frame) }
    fn transmit(&mut self,frame:WpsFrame)->Result<(),WpsError> {self.push(WpsEffect::Transmit{peer:self.peer.ok_or(WpsError::Peer)?,frame})}
    pub fn receive_eapol(&mut self,peer:MacAddress,input:&[u8])->Result<(),WpsError> {
        if self.peer!=Some(peer){return Err(WpsError::Peer);}if matches!(self.state,WpsState::Finished|WpsState::Failed){return Err(WpsError::State);} let result=self.receive(input); if let Err(error)=result {self.fail(error)?;} result
    }
    fn receive(&mut self,input:&[u8])->Result<(),WpsError> {
        if input.len()<8 || input[1]!=0{return Err(WpsError::Truncated);} let outer=u16::from_be_bytes([input[2],input[3]]) as usize;
        if outer+4!=input.len() || u16::from_be_bytes([input[6],input[7]]) as usize!=outer{return Err(WpsError::Truncated);}
        if input[4]==4 {return Err(WpsError::Registrar);} if input[4]!=1 || input.len()<9{return Err(WpsError::Message);} let id=input[5];
        if self.last_id==Some(id) {if let Some(frame)=self.last_response.clone(){return self.transmit(frame);}}
        if input[8]==1 {
            let identity=b"WFA-SimpleConfig-Enrollee-1-0";let length=5+identity.len();let mut frame=WpsFrame::new();frame.append(&[1,0])?;frame.append(&(length as u16).to_be_bytes())?;frame.append(&[2,id])?;frame.append(&(length as u16).to_be_bytes())?;frame.append(&[1])?;frame.append(identity)?;return self.respond(id,frame);
        }
        if input.len()<18 || input[8..16]!=[254,0,0x37,0x2a,0,0,0,1]{return Err(WpsError::Message);} let opcode=input[16];let flags=input[17]; if flags&!3!=0{return Err(WpsError::Message);}
        if opcode==6 {if self.outgoing_used>=self.outgoing.as_bytes().len(){return Err(WpsError::State);}return self.emit_fragment(id);}
        if opcode==1 {if self.state!=WpsState::Start{return Err(WpsError::State);}let message=self.m1()?;self.state=WpsState::M2;return self.send_message(id,4,message,true);}
        if opcode!=4 && opcode!=3{return Err(WpsError::Message);}
        let mut payload=&input[18..];
        if flags&2!=0 {if payload.len()<2 || self.incoming_total.is_some(){return Err(WpsError::Truncated);}let total=u16::from_be_bytes([payload[0],payload[1]]) as usize;if total>wire::CAPACITY{return Err(WpsError::Capacity);}self.incoming_total=Some(total);self.incoming_opcode=opcode;payload=&payload[2..];}
        if self.incoming_total.is_some() {if self.incoming_opcode!=opcode{return Err(WpsError::Message);}self.incoming.append(payload)?;if self.incoming.as_bytes().len()>self.incoming_total.unwrap_or(0){return Err(WpsError::Truncated);}if flags&1!=0 {return self.respond(id,wire::eap_response(id,6,0,&[],None)?);}if self.incoming.as_bytes().len()!=self.incoming_total.unwrap_or(0){return Err(WpsError::Truncated);}let message=core::mem::take(&mut self.incoming);self.incoming_total=None;self.process_message(id,opcode,message.as_bytes())}
        else {if flags&1!=0{return Err(WpsError::Truncated);}self.process_message(id,opcode,payload)}
    }
    fn respond(&mut self,id:u8,frame:WpsFrame)->Result<(),WpsError> {self.last_id=Some(id);self.last_response=Some(frame.clone());self.transmit(frame)}
    fn send_message(&mut self,id:u8,opcode:u8,message:WpsFrame,remember:bool)->Result<(),WpsError> {if remember{self.last=message.clone();}self.outgoing=message;self.outgoing_used=0;self.outgoing_opcode=opcode;self.emit_fragment(id)}
    fn emit_fragment(&mut self,id:u8)->Result<(),WpsError> {
        let bytes=self.outgoing.as_bytes();let remaining=bytes.len()-self.outgoing_used;let count=remaining.min(480);let first=self.outgoing_used==0;let more=remaining>count;let flags=if more{1}else{0}|if first&&more{2}else{0};
        let frame=wire::eap_response(id,self.outgoing_opcode,flags,&bytes[self.outgoing_used..self.outgoing_used+count],if first&&more{Some(bytes.len())}else{None})?;self.outgoing_used+=count;self.respond(id,frame)
    }
    fn m1(&self)->Result<WpsFrame,WpsError> {
        let mut msg=WpsFrame::message(M::M1)?;let mut uuid=[0;16];uuid[10..].copy_from_slice(&self.local.0);
        msg.attribute(A::UuidE,&uuid)?;msg.attribute(A::Mac,&self.local.0)?;msg.attribute(A::EnrolleeNonce,&self.nonce.0)?;msg.attribute(A::PublicKey,self.public.as_ref().ok_or(WpsError::State)?.as_bytes())?;
        msg.attribute(A::AuthFlags,&0x0023u16.to_be_bytes())?;msg.attribute(A::EncryptionFlags,&0x000fu16.to_be_bytes())?;msg.attribute(A::ConnectionFlags,&[1])?;
        msg.attribute(A::ConfigMethods,&0x008cu16.to_be_bytes())?;msg.attribute(A::WpsState,&[1])?;
        msg.attribute(A::Manufacturer,b"FUJIFILM")?;msg.attribute(A::ModelName,b"X-M1")?;msg.attribute(A::ModelNumber,b"X-M1")?;msg.attribute(A::Serial,b"")?;msg.attribute(A::DeviceName,b"X-M1")?;msg.attribute(A::PrimaryDeviceType,&[0,6,0,0x50,0xf2,4,0,1])?;
        msg.attribute(A::RfBands,&[1])?;msg.attribute(A::AssociationState,&[0,0])?;msg.attribute(A::PasswordId,&if self.configuration.method==WpsMethod::Pbc{4u16}else{0u16}.to_be_bytes())?;msg.attribute(A::ConfigError,&[0,0])?;msg.attribute(A::OsVersion,&[0x80,0,0,0])?;Ok(msg)
    }
    fn control(&self,kind:M)->Result<WpsFrame,WpsError> {let mut msg=WpsFrame::message(kind)?;msg.attribute(A::EnrolleeNonce,&self.nonce.0)?;msg.attribute(A::RegistrarNonce,&self.registrar_nonce.0)?;Ok(msg)}
    fn authenticate(&self,msg:&mut WpsFrame,previous:&[u8])->Result<(),WpsError> {let auth=self.keys.as_ref().ok_or(WpsError::State)?.authenticator(&[previous,msg.as_bytes()]);msg.attribute(A::Authenticator,&auth.0)}
    fn process_message(&mut self,id:u8,opcode:u8,input:&[u8])->Result<(),WpsError> {
        let attrs=Attributes::parse(input)?;let kind=M::parse(attrs.get(A::MessageType)?)?;
        if attrs.fixed::<16>(A::EnrolleeNonce)?!=self.nonce.0{return Err(WpsError::Authentication);}
        if opcode==3 || kind==M::Nack {return Err(WpsError::Registrar);}
        if kind==M::M2d && self.state==WpsState::M2 {self.registrar_nonce=Nonce(attrs.fixed(A::RegistrarNonce)?);let ack=self.control(M::Ack)?;return self.send_message(id,2,ack,false);}
        match (self.state,kind) {
            (WpsState::M2,M::M2)=>{
                self.registrar_nonce=Nonce(attrs.fixed(A::RegistrarNonce)?);let _=attrs.fixed::<16>(A::UuidR)?;
                let registrar=DhPublic::from_bytes(attrs.fixed(A::PublicKey)?).map_err(|_|WpsError::Crypto)?;
                self.keys=Some(self.private.take().ok_or(WpsError::State)?.derive_keys(&registrar,&self.nonce,&EnrolleeMac(self.local.0),&self.registrar_nonce).map_err(|_|WpsError::Crypto)?);self.registrar=Some(registrar);
                self.verify(&attrs)?;
                let password=match &self.configuration.method {WpsMethod::Pbc=>&b"00000000"[..],WpsMethod::Pin(pin)=>pin.as_bytes(),WpsMethod::GeneratePin=>return Err(WpsError::State)};
                let proofs=self.keys.as_ref().ok_or(WpsError::State)?.password_proofs(password).map_err(|_|WpsError::Crypto)?;
                let public=self.public.as_ref().ok_or(WpsError::State)?;let registrar=self.registrar.as_ref().ok_or(WpsError::State)?;
                let hash1=proofs.commitment(&self.secret1,public,registrar,PasswordHalf::First);let hash2=proofs.commitment(&self.secret2,public,registrar,PasswordHalf::Second);drop(proofs);
                let mut reply=WpsFrame::message(M::M3)?;reply.attribute(A::RegistrarNonce,&self.registrar_nonce.0)?;reply.attribute(A::EHash1,&hash1.0)?;reply.attribute(A::EHash2,&hash2.0)?;self.authenticate(&mut reply,input)?;self.state=WpsState::M4;self.send_message(id,4,reply,true)
            }
            (WpsState::M4,M::M4)|(WpsState::M6,M::M6)|(WpsState::M8,M::M8)=>{
                self.verify(&attrs)?;let mut plain=WpsFrame::new();let mut decrypted=[0;wire::CAPACITY];let length=self.keys.as_ref().ok_or(WpsError::State)?.decrypt_settings(attrs.get(A::EncryptedSettings)?,&mut decrypted).map_err(|_|WpsError::Authentication)?;plain.append(&decrypted[..length])?;decrypted.fill(0);let settings=Attributes::parse(plain.as_bytes())?;
                if kind==M::M8 {let mut selected=None;for (tag,value) in settings.all(){if tag==A::Credential as u16{selected=Some(parse_credential(value)?);}}let credential=selected.ok_or(WpsError::Credential)?;let profile=credential.profile.clone();self.credential=Some(credential);let done=self.control(M::Done)?;self.send_message(id,5,done,false)?;self.finish(WpsOutcome::Credentials(profile))}
                else {
                    let (nonce,expected,half,send,secret,attribute,next)=if kind==M::M4 {self.hash1=Commitment(attrs.fixed(A::RHash1)?);self.hash2=Commitment(attrs.fixed(A::RHash2)?);(settings.fixed::<16>(A::RSnonce1)?,self.hash1,PasswordHalf::First,M::M5,&self.secret1,A::ESnonce1,WpsState::M6)}else{(settings.fixed::<16>(A::RSnonce2)?,self.hash2,PasswordHalf::Second,M::M7,&self.secret2,A::ESnonce2,WpsState::M8)};
                    let password=match &self.configuration.method {WpsMethod::Pbc=>&b"00000000"[..],WpsMethod::Pin(pin)=>pin.as_bytes(),WpsMethod::GeneratePin=>return Err(WpsError::State)};
                    let proofs=self.keys.as_ref().ok_or(WpsError::State)?.password_proofs(password).map_err(|_|WpsError::Crypto)?;
                    if !proofs.verify_commitment(&SecretNonce::from_bytes(nonce),self.public.as_ref().ok_or(WpsError::State)?,self.registrar.as_ref().ok_or(WpsError::State)?,half,&expected){return Err(WpsError::Authentication);}drop(proofs);
                    let mut settings=WpsFrame::new();settings.attribute(attribute,secret.as_bytes())?;let mut encrypted=[0;wire::CAPACITY];let length=self.keys.as_ref().ok_or(WpsError::State)?.encrypt_settings(if kind==M::M4{&self.iv}else{&self.second_iv},settings.as_bytes(),&mut encrypted).map_err(|_|WpsError::Crypto)?;
                    let mut reply=WpsFrame::message(send)?;reply.attribute(A::RegistrarNonce,&self.registrar_nonce.0)?;reply.attribute(A::EncryptedSettings,&encrypted[..length])?;encrypted.fill(0);self.authenticate(&mut reply,input)?;self.state=next;self.send_message(id,4,reply,true)
                }
            }
            _=>Err(WpsError::State),
        }
    }
    fn verify(&self,attrs:&Attributes<'_>)->Result<(),WpsError> {let (body,auth)=attrs.authenticated()?;if !self.keys.as_ref().ok_or(WpsError::State)?.verify_authenticator(&[self.last.as_bytes(),body],&Authenticator(auth)){return Err(WpsError::Authentication);}Ok(())}
    pub fn timeout(&mut self)->Result<(),WpsError> {if matches!(self.state,WpsState::Finished|WpsState::Failed|WpsState::Idle){return Err(WpsError::State);}self.fail(WpsError::Timeout)}
    pub fn cancel(&mut self) {for effect in &mut self.effects{*effect=None;}self.queued=0;self.credential=None;let _=self.finish(WpsOutcome::Cancelled);}
    fn fail(&mut self,error:WpsError)->Result<(),WpsError> {for effect in &mut self.effects{*effect=None;}self.queued=0;if let (Some(id),Some(_))=(self.last_id,self.peer){let mut nack=self.control(M::Nack)?;nack.attribute(A::ConfigError,&18u16.to_be_bytes())?;self.send_message(id,3,nack,false)?;}self.finish(WpsOutcome::Failed(error))}
    fn finish(&mut self,outcome:WpsOutcome)->Result<(),WpsError> {self.state=if matches!(outcome,WpsOutcome::Credentials(_)){WpsState::Finished}else{WpsState::Failed};self.outcome=Some(outcome);self.private=None;self.keys=None;self.entropy.fill(0);self.secret1=SecretNonce::from_bytes([0;16]);self.secret2=SecretNonce::from_bytes([0;16]);self.iv.0.fill(0);self.second_iv.0.fill(0);self.last.clear();self.incoming.clear();self.outgoing.clear();self.last_response=None;self.push(WpsEffect::CancelTimer)?;self.push(WpsEffect::Disconnect)}
}
impl Drop for WpsEnrollee {fn drop(&mut self){self.entropy.fill(0);self.iv.0.fill(0);self.second_iv.0.fill(0);}}
fn parse_credential(input:&[u8])->Result<WpsCredential,WpsError> {
    let attrs=Attributes::parse(input)?;let ssid=Ssid::parse(attrs.get(A::Ssid)?).map_err(|_|WpsError::Credential)?;let auth=attrs.word(A::AuthType)?;let encryption=attrs.word(A::Encryption)?;let _=attrs.fixed::<6>(A::Mac)?;
    let key=attrs.optional(A::NetworkKey)?.unwrap_or(&[]);let index=match attrs.optional(A::NetworkKeyIndex)? {Some([index]) if (1..=4).contains(index)=>*index-1,None=>0,_=>return Err(WpsError::Credential)};
    let security=if auth&0x0020!=0 && encryption&0x0008!=0{WpsSecurity::Wpa2}else if auth&0x0002!=0 && encryption&(0x0004|0x0008)!=0{WpsSecurity::Wpa}else if auth&(0x0001|0x0004)!=0 && encryption==2{WpsSecurity::Wep}else if auth==1 && encryption==1{WpsSecurity::Open}else{return Err(WpsError::Credential)};
    let profile=StationProfile::parse(ssid,security as u8,key,index).map_err(|_|WpsError::Credential)?;
    if key.len()>64{return Err(WpsError::Credential);}let mut bytes=[0;64];bytes[..key.len()].copy_from_slice(key);
    Ok(WpsCredential{profile,key:WpsNetworkKey{bytes,length:key.len() as u8},security,index:super::KeyIndex::parse(index).map_err(|_|WpsError::Credential)?})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn cancellation_discards_queued_entropy_and_never_installs_credentials(){let mut enrollee=WpsEnrollee::new(MacAddress([2;6]),WpsConfiguration{method:WpsMethod::Pbc,bssid:None}).unwrap();enrollee.start().unwrap();enrollee.cancel();assert_eq!(enrollee.take_outcome(),Some(WpsOutcome::Cancelled));assert_eq!(enrollee.supply_entropy([1;32]),Err(WpsError::State));}
    #[test] fn credentials_decode_into_actual_station_profile(){let mut credential=WpsFrame::new();credential.attribute(A::Ssid,b"synthetic").unwrap();credential.attribute(A::AuthType,&[0,0x20]).unwrap();credential.attribute(A::Encryption,&[0,8]).unwrap();credential.attribute(A::Mac,&[2;6]).unwrap();credential.attribute(A::NetworkKey,b"test password").unwrap();let credential=parse_credential(credential.as_bytes()).unwrap();assert_eq!(credential.ssid().as_bytes(),b"synthetic");assert_eq!(credential.key(),b"test password");assert!(matches!(credential.profile.credentials,super::super::Credentials::Personal(_)));}
    #[test] fn wrong_peer_cannot_advance_session(){let mut enrollee=WpsEnrollee::new(MacAddress([2;6]),WpsConfiguration{method:WpsMethod::Pbc,bssid:Some(MacAddress([3;6]))}).unwrap();assert_eq!(enrollee.receive_eapol(MacAddress([4;6]),&[]),Err(WpsError::Peer));assert_eq!(enrollee.state(),WpsState::Idle);}
}
