//! X-M1 1.50 registrar protocol, x70view 55436fc85c0560dd252b0410af6f2427fe198cd3.
//! Message handlers 0x01377e74, 0x01378380, 0x0137850c and 0x01378a58.

use super::{wire, A, M, Attributes, WpsFrame, WpsError, WpsSecurity, WpsCredential, WpsNetworkKey, WpsPin, MacAddress, Ssid, StationProfile, DhPrivate, DhPublic, Nonce, SecretNonce, Authenticator, Commitment, EnrolleeMac, Iv, WpsKeys, PasswordHalf};
#[cfg(test)]
#[path="wireless_station_wps_registrar_tests.rs"]
mod tests;

#[derive(Clone,Debug)]
pub struct RegistrarCredentials(WpsCredential);
impl RegistrarCredentials {
    pub fn new(ssid:Ssid,security:WpsSecurity,key:&[u8],index:u8)->Result<Self,WpsError>{
        let profile=StationProfile::parse(ssid,security as u8,key,index).map_err(|_|WpsError::Credential)?;
        if key.len()>64{return Err(WpsError::Credential);}let mut bytes=[0;64];bytes[..key.len()].copy_from_slice(key);
        let index=super::super::KeyIndex::parse(index).map_err(|_|WpsError::Credential)?;
        Ok(Self(WpsCredential{profile,key:WpsNetworkKey{bytes,length:key.len() as u8},security,index}))
    }
}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum RegistrarState {Idle,Entropy,Waiting,Identity,M1,M3,M5,M7,Done,Finished,Failed}
#[derive(Clone,Copy,Debug,Eq,PartialEq)]
pub enum RegistrarOutcome {Registered{peer:MacAddress},Cancelled,Failed(WpsError)}
#[derive(Debug)]
pub enum RegistrarEffect {Entropy,Transmit{peer:MacAddress,frame:WpsFrame},ArmTimer{milliseconds:u32},CancelTimer,SelectedRegistrar{elements:WpsFrame},Disconnect{peer:MacAddress}}
#[derive(Clone,Copy)]
enum Method {Pbc,Pin(WpsPin)}
pub struct Registrar {
    local:MacAddress, credentials:RegistrarCredentials, state:RegistrarState, method:Option<Method>, allowed:Option<MacAddress>, peer:Option<MacAddress>,
    effects:[Option<RegistrarEffect>;8], queued:usize,outcome:Option<RegistrarOutcome>,entropy:[u8;288],entropy_used:usize,
    private:Option<DhPrivate>,public:Option<DhPublic>,enrollee_public:Option<DhPublic>,keys:Option<WpsKeys>,
    nonce:Nonce,enrollee_nonce:Nonce,secret1:SecretNonce,secret2:SecretNonce,ivs:[Iv;3],ehash1:Commitment,ehash2:Commitment,
    identifier:u8,last_request:Option<WpsFrame>,last_response:WpsFrame,last:WpsFrame,
    outgoing:WpsFrame,outgoing_used:usize,outgoing_opcode:u8,incoming:WpsFrame,incoming_total:Option<usize>,incoming_opcode:u8,
}
impl Registrar {
    pub fn new(local:MacAddress,credentials:RegistrarCredentials)->Self {
        Self{local,credentials,state:RegistrarState::Idle,method:None,allowed:None,peer:None,effects:core::array::from_fn(|_|None),queued:0,outcome:None,entropy:[0;288],entropy_used:0,private:None,public:None,enrollee_public:None,keys:None,nonce:Nonce([0;16]),enrollee_nonce:Nonce([0;16]),secret1:SecretNonce::from_bytes([0;16]),secret2:SecretNonce::from_bytes([0;16]),ivs:[Iv([0;16]);3],ehash1:Commitment([0;32]),ehash2:Commitment([0;32]),identifier:0,last_request:None,last_response:WpsFrame::new(),last:WpsFrame::new(),outgoing:WpsFrame::new(),outgoing_used:0,outgoing_opcode:0,incoming:WpsFrame::new(),incoming_total:None,incoming_opcode:0}
    }
    pub const fn state(&self)->RegistrarState{self.state}
    pub const fn is_drained(&self)->bool{self.queued==0}
    pub fn start_pbc(&mut self,peer:Option<MacAddress>)->Result<(),WpsError>{self.start(Method::Pbc,peer)}
    pub fn start_pin(&mut self,pin:WpsPin,peer:Option<MacAddress>)->Result<(),WpsError>{self.start(Method::Pin(pin),peer)}
    fn start(&mut self,method:Method,peer:Option<MacAddress>)->Result<(),WpsError>{if self.queued!=0||!matches!(self.state,RegistrarState::Idle|RegistrarState::Finished|RegistrarState::Failed){return Err(WpsError::State);}self.clear();self.method=Some(method);self.allowed=peer;self.peer=None;self.outcome=None;self.state=RegistrarState::Entropy;self.entropy_used=0;self.push(RegistrarEffect::Entropy)}
    pub fn poll(&mut self)->Option<RegistrarEffect>{if self.queued==0{return None;}let effect=self.effects[0].take();for index in 1..self.queued{self.effects[index-1]=self.effects[index].take();}self.queued-=1;effect}
    pub fn take_outcome(&mut self)->Option<RegistrarOutcome>{self.outcome.take()}
    fn push(&mut self,effect:RegistrarEffect)->Result<(),WpsError>{if self.queued==self.effects.len(){return Err(WpsError::Capacity);}self.effects[self.queued]=Some(effect);self.queued+=1;Ok(())}
    pub fn supply_entropy(&mut self,mut bytes:[u8;32])->Result<(),WpsError>{
        if self.state!=RegistrarState::Entropy{bytes.fill(0);return Err(WpsError::State);}let size=(self.entropy.len()-self.entropy_used).min(32);self.entropy[self.entropy_used..self.entropy_used+size].copy_from_slice(&bytes[..size]);bytes.fill(0);self.entropy_used+=size;
        if self.entropy_used<self.entropy.len(){return self.push(RegistrarEffect::Entropy);}
        let private=match DhPrivate::from_bytes(self.entropy[..192].try_into().map_err(|_|WpsError::Crypto)?){Ok(private)=>private,Err(_)=>{self.entropy.fill(0);self.entropy_used=0;return self.push(RegistrarEffect::Entropy);}};
        self.public=Some(private.public_key());self.private=Some(private);self.nonce=Nonce(self.entropy[192..208].try_into().map_err(|_|WpsError::Crypto)?);self.secret1=SecretNonce::from_bytes(self.entropy[208..224].try_into().map_err(|_|WpsError::Crypto)?);self.secret2=SecretNonce::from_bytes(self.entropy[224..240].try_into().map_err(|_|WpsError::Crypto)?);
        self.ivs[0]=Iv(self.entropy[240..256].try_into().map_err(|_|WpsError::Crypto)?);self.ivs[1]=Iv(self.entropy[256..272].try_into().map_err(|_|WpsError::Crypto)?);self.ivs[2]=Iv(self.entropy[272..288].try_into().map_err(|_|WpsError::Crypto)?);
        self.entropy.fill(0);self.entropy_used=0;self.state=RegistrarState::Waiting;self.advertise(true)?;self.push(RegistrarEffect::ArmTimer{milliseconds:120_000})
    }
    pub fn initial_advertisement(&self)->Result<WpsFrame,WpsError>{self.advertisement(false)}
    fn advertise(&mut self,selected:bool)->Result<(),WpsError>{let elements=self.advertisement(selected)?;self.push(RegistrarEffect::SelectedRegistrar{elements})}
    fn advertisement(&self,selected:bool)->Result<WpsFrame,WpsError>{
        let mut attrs=WpsFrame::new();attrs.attribute(A::Version,&[0x10])?;attrs.attribute(A::WpsState,&[2])?;attrs.attribute(A::ApSetupLocked,&[1])?;attrs.attribute(A::SelectedRegistrar,&[selected as u8])?;
        if selected{attrs.attribute(A::PasswordId,&self.password_id().to_be_bytes())?;attrs.attribute(A::SelectedRegistrarConfigMethods,&if matches!(self.method,Some(Method::Pbc)){0x0080u16}else{0x000cu16}.to_be_bytes())?;}
        let mut elements=WpsFrame::new();for chunk in attrs.as_bytes().chunks(251){elements.append(&[221,(chunk.len()+4) as u8,0,0x50,0xf2,4])?;elements.append(chunk)?;}Ok(elements)
    }
    fn password_id(&self)->u16{if matches!(self.method,Some(Method::Pbc)){4}else{0}}
    fn password(&self)->Result<&[u8],WpsError>{match &self.method{Some(Method::Pbc)=>Ok(b"00000000"),Some(Method::Pin(pin))=>Ok(pin.as_bytes()),None=>Err(WpsError::State)}}
    pub fn receive_eapol(&mut self,peer:MacAddress,input:&[u8])->Result<(),WpsError>{
        if matches!(self.state,RegistrarState::Idle|RegistrarState::Entropy|RegistrarState::Finished|RegistrarState::Failed){return Err(WpsError::State);}
        if self.allowed.is_some_and(|allowed|allowed!=peer){return Err(WpsError::Peer);}if self.peer.is_some_and(|active|active!=peer){if matches!(self.method,Some(Method::Pbc)){self.fail(WpsError::Overlap)?;return Err(WpsError::Overlap);}return Err(WpsError::Peer);}
        self.peer=Some(peer);
        let result=self.receive(input);if let Err(error)=result{self.fail(error)?;}result
    }
    fn receive(&mut self,input:&[u8])->Result<(),WpsError>{
        if input==[1,1,0,0]||input==[2,1,0,0]{if let Some(frame)=self.last_request.clone(){return self.transmit(frame);}self.state=RegistrarState::Identity;return self.identity();}
        if input.len()<9||input[1]!=0||input[4]!=2{return Err(WpsError::Message);}let length=u16::from_be_bytes([input[2],input[3]]) as usize;if length+4!=input.len()||u16::from_be_bytes([input[6],input[7]]) as usize!=length{return Err(WpsError::Truncated);}
        if input==self.last_response.as_bytes(){if let Some(frame)=self.last_request.clone(){return self.transmit(frame);}}
        if input[5]!=self.identifier{return Err(WpsError::State);}self.last_response.clear();self.last_response.append(input)?;
        if self.state==RegistrarState::Identity{if input[8]!=1||&input[9..]!=b"WFA-SimpleConfig-Enrollee-1-0"{return Err(WpsError::Message);}self.state=RegistrarState::M1;return self.send(1,WpsFrame::new(),false);}
        if input.len()<18||input[8..16]!=[254,0,0x37,0x2a,0,0,0,1]{return Err(WpsError::Message);}let opcode=input[16];let flags=input[17];if flags&!3!=0{return Err(WpsError::Message);}
        if opcode==6{if self.outgoing_used>=self.outgoing.as_bytes().len(){return Err(WpsError::State);}return self.emit_fragment();}
        if opcode!=4&&opcode!=5&&opcode!=3{return Err(WpsError::Message);}let mut payload=&input[18..];
        if flags&2!=0{if payload.len()<2||self.incoming_total.is_some(){return Err(WpsError::Truncated);}let length=u16::from_be_bytes([payload[0],payload[1]]) as usize;if length>wire::CAPACITY{return Err(WpsError::Capacity);}self.incoming_total=Some(length);self.incoming_opcode=opcode;payload=&payload[2..];}
        if self.incoming_total.is_some(){if opcode!=self.incoming_opcode{return Err(WpsError::Message);}self.incoming.append(payload)?;if self.incoming.as_bytes().len()>self.incoming_total.unwrap_or(0){return Err(WpsError::Truncated);}if flags&1!=0{return self.request(6,0,&[],None);}if self.incoming.as_bytes().len()!=self.incoming_total.unwrap_or(0){return Err(WpsError::Truncated);}let message=core::mem::take(&mut self.incoming);self.incoming_total=None;self.process(opcode,message.as_bytes())}else{if flags&1!=0{return Err(WpsError::Truncated);}self.process(opcode,payload)}
    }
    fn identity(&mut self)->Result<(),WpsError>{self.identifier=self.identifier.wrapping_add(1);let mut frame=WpsFrame::new();frame.append(&[1,0,0,5,1,self.identifier,0,5,1])?;self.last_request=Some(frame.clone());self.transmit(frame)}
    fn transmit(&mut self,frame:WpsFrame)->Result<(),WpsError>{self.push(RegistrarEffect::Transmit{peer:self.peer.ok_or(WpsError::Peer)?,frame})}
    fn request(&mut self,opcode:u8,flags:u8,payload:&[u8],total:Option<usize>)->Result<(),WpsError>{self.identifier=self.identifier.wrapping_add(1);let frame=wire::eap_packet(1,self.identifier,opcode,flags,payload,total)?;self.last_request=Some(frame.clone());self.transmit(frame)}
    fn send(&mut self,opcode:u8,message:WpsFrame,remember:bool)->Result<(),WpsError>{if remember{self.last=message.clone();}self.outgoing=message;self.outgoing_used=0;self.outgoing_opcode=opcode;self.emit_fragment()}
    fn emit_fragment(&mut self)->Result<(),WpsError>{let remaining=self.outgoing.as_bytes().len()-self.outgoing_used;let count=remaining.min(480);let more=remaining>count;let first=self.outgoing_used==0;let flags=(if more{1}else{0})|(if first&&more{2}else{0});let mut payload=WpsFrame::new();payload.append(&self.outgoing.as_bytes()[self.outgoing_used..self.outgoing_used+count])?;self.outgoing_used+=count;self.request(self.outgoing_opcode,flags,payload.as_bytes(),if first&&more{Some(self.outgoing.as_bytes().len())}else{None})}
    fn verify(&self,attrs:&Attributes<'_>)->Result<(),WpsError>{if attrs.fixed::<16>(A::RegistrarNonce)?!=self.nonce.0{return Err(WpsError::Authentication);}let (unsigned,auth)=attrs.authenticated()?;if !self.keys.as_ref().ok_or(WpsError::State)?.verify_authenticator(&[self.last.as_bytes(),unsigned],&Authenticator(auth)){return Err(WpsError::Authentication);}Ok(())}
    fn authenticate(&self,message:&mut WpsFrame,previous:&[u8])->Result<(),WpsError>{let auth=self.keys.as_ref().ok_or(WpsError::State)?.authenticator(&[previous,message.as_bytes()]);message.attribute(A::Authenticator,&auth.0)}
    fn response(&self,kind:M)->Result<WpsFrame,WpsError>{let mut message=WpsFrame::message(kind)?;message.attribute(A::EnrolleeNonce,&self.enrollee_nonce.0)?;Ok(message)}
    fn encrypt(&self,message:&mut WpsFrame,plain:&WpsFrame,index:usize)->Result<(),WpsError>{let mut encrypted=[0;wire::CAPACITY];let length=self.keys.as_ref().ok_or(WpsError::State)?.encrypt_settings(&self.ivs[index],plain.as_bytes(),&mut encrypted).map_err(|_|WpsError::Crypto)?;message.attribute(A::EncryptedSettings,&encrypted[..length])?;encrypted.fill(0);Ok(())}
    fn process(&mut self,opcode:u8,input:&[u8])->Result<(),WpsError>{
        let attrs=Attributes::parse(input)?;let kind=M::parse(attrs.get(A::MessageType)?)?;if opcode==3||kind==M::Nack{return Err(WpsError::Registrar);}
        if kind!=M::Done&&opcode!=4{return Err(WpsError::Message);}
        match (self.state,kind){
            (RegistrarState::M1,M::M1)=>{
                if attrs.fixed::<6>(A::Mac)?!=self.peer.ok_or(WpsError::Peer)?.0{return Err(WpsError::Peer);}self.enrollee_nonce=Nonce(attrs.fixed(A::EnrolleeNonce)?);let _=attrs.fixed::<16>(A::UuidE)?;
                if attrs.word(A::PasswordId)?!=self.password_id(){return Err(WpsError::Authentication);}let enrollee=DhPublic::from_bytes(attrs.fixed(A::PublicKey)?).map_err(|_|WpsError::Crypto)?;
                let credential=&self.credentials.0;let (auth,encryption)=security_flags(credential.security);if attrs.word(A::AuthFlags)?&auth==0||attrs.word(A::EncryptionFlags)?&encryption==0{return Err(WpsError::Credential);}
                self.keys=Some(self.private.take().ok_or(WpsError::State)?.derive_keys(&enrollee,&self.enrollee_nonce,&EnrolleeMac(self.peer.ok_or(WpsError::Peer)?.0),&self.nonce).map_err(|_|WpsError::Crypto)?);self.enrollee_public=Some(enrollee);
                let mut message=self.response(M::M2)?;message.attribute(A::RegistrarNonce,&self.nonce.0)?;let mut uuid=[0;16];uuid[10..].copy_from_slice(&self.local.0);message.attribute(A::UuidR,&uuid)?;message.attribute(A::PublicKey,self.public.as_ref().ok_or(WpsError::State)?.as_bytes())?;
                message.attribute(A::AuthFlags,&auth.to_be_bytes())?;message.attribute(A::EncryptionFlags,&encryption.to_be_bytes())?;message.attribute(A::ConnectionFlags,&[1])?;message.attribute(A::ConfigMethods,&0x008cu16.to_be_bytes())?;
                message.attribute(A::Manufacturer,b"FUJIFILM")?;message.attribute(A::ModelName,b"X-M1")?;message.attribute(A::ModelNumber,b"X-M1")?;message.attribute(A::Serial,b"")?;message.attribute(A::DeviceName,b"X-M1")?;message.attribute(A::PrimaryDeviceType,&[0,6,0,0x50,0xf2,4,0,1])?;message.attribute(A::RfBands,&[1])?;message.attribute(A::AssociationState,&[0,0])?;message.attribute(A::ConfigError,&[0,0])?;message.attribute(A::PasswordId,&self.password_id().to_be_bytes())?;message.attribute(A::OsVersion,&[0x80,0,0,0])?;
                self.authenticate(&mut message,input)?;self.state=RegistrarState::M3;self.send(4,message,true)
            }
            (RegistrarState::M3,M::M3)=>{
                self.verify(&attrs)?;self.ehash1=Commitment(attrs.fixed(A::EHash1)?);self.ehash2=Commitment(attrs.fixed(A::EHash2)?);
                let proofs=self.keys.as_ref().ok_or(WpsError::State)?.password_proofs(self.password()?).map_err(|_|WpsError::Crypto)?;let enrollee=self.enrollee_public.as_ref().ok_or(WpsError::State)?;let public=self.public.as_ref().ok_or(WpsError::State)?;
                let hash1=proofs.commitment(&self.secret1,enrollee,public,PasswordHalf::First);let hash2=proofs.commitment(&self.secret2,enrollee,public,PasswordHalf::Second);drop(proofs);
                let mut message=self.response(M::M4)?;message.attribute(A::RHash1,&hash1.0)?;message.attribute(A::RHash2,&hash2.0)?;let mut plain=WpsFrame::new();plain.attribute(A::RSnonce1,self.secret1.as_bytes())?;self.encrypt(&mut message,&plain,0)?;self.authenticate(&mut message,input)?;self.state=RegistrarState::M5;self.send(4,message,true)
            }
            (RegistrarState::M5,M::M5)|(RegistrarState::M7,M::M7)=>{
                self.verify(&attrs)?;let mut decrypted=[0;wire::CAPACITY];let length=self.keys.as_ref().ok_or(WpsError::State)?.decrypt_settings(attrs.get(A::EncryptedSettings)?,&mut decrypted).map_err(|_|WpsError::Authentication)?;let mut plain=WpsFrame::new();plain.append(&decrypted[..length])?;decrypted.fill(0);let settings=Attributes::parse(plain.as_bytes())?;
                let (attribute,half,expected)=if kind==M::M5{(A::ESnonce1,PasswordHalf::First,&self.ehash1)}else{(A::ESnonce2,PasswordHalf::Second,&self.ehash2)};let nonce=SecretNonce::from_bytes(settings.fixed(attribute)?);
                let proofs=self.keys.as_ref().ok_or(WpsError::State)?.password_proofs(self.password()?).map_err(|_|WpsError::Crypto)?;if !proofs.verify_commitment(&nonce,self.enrollee_public.as_ref().ok_or(WpsError::State)?,self.public.as_ref().ok_or(WpsError::State)?,half,expected){return Err(WpsError::Authentication);}drop(proofs);
                let mut message=self.response(if kind==M::M5{M::M6}else{M::M8})?;let mut settings=WpsFrame::new();
                if kind==M::M5{settings.attribute(A::RSnonce2,self.secret2.as_bytes())?;self.encrypt(&mut message,&settings,1)?;self.state=RegistrarState::M7;}else{let credential=self.credential_bytes()?;settings.attribute(A::Credential,credential.as_bytes())?;self.encrypt(&mut message,&settings,2)?;self.state=RegistrarState::Done;}
                self.authenticate(&mut message,input)?;self.send(4,message,true)
            }
            (RegistrarState::Done,M::Done)=>{
                if opcode!=5||attrs.fixed::<16>(A::EnrolleeNonce)?!=self.enrollee_nonce.0||attrs.fixed::<16>(A::RegistrarNonce)?!=self.nonce.0{return Err(WpsError::Authentication);}let mut failure=WpsFrame::new();failure.append(&[1,0,0,4,4,self.identifier,0,4])?;self.transmit(failure)?;self.finish(RegistrarOutcome::Registered{peer:self.peer.ok_or(WpsError::Peer)?})
            }
            _=>Err(WpsError::State),
        }
    }
    fn credential_bytes(&self)->Result<WpsFrame,WpsError>{let credential=&self.credentials.0;let (auth,encryption)=security_flags(credential.security);let mut message=WpsFrame::new();message.attribute(A::NetworkIndex,&[1])?;message.attribute(A::Ssid,credential.ssid().as_bytes())?;message.attribute(A::AuthType,&auth.to_be_bytes())?;message.attribute(A::Encryption,&encryption.to_be_bytes())?;if credential.security==WpsSecurity::Wep{message.attribute(A::NetworkKeyIndex,&[credential.key_index()+1])?;}message.attribute(A::NetworkKey,credential.key())?;message.attribute(A::Mac,&self.peer.ok_or(WpsError::Peer)?.0)?;Ok(message)}
    pub fn timeout(&mut self)->Result<(),WpsError>{if matches!(self.state,RegistrarState::Idle|RegistrarState::Entropy|RegistrarState::Finished|RegistrarState::Failed){return Err(WpsError::State);}if matches!(self.method,Some(Method::Pin(_))){self.advertise(false)}else{self.fail(WpsError::Timeout)}}
    pub fn retry(&mut self)->Result<(),WpsError>{let frame=self.last_request.clone().ok_or(WpsError::State)?;self.transmit(frame)}
    pub fn cancel(&mut self){for effect in &mut self.effects{*effect=None;}self.queued=0;let _=self.finish(RegistrarOutcome::Cancelled);}
    fn fail(&mut self,error:WpsError)->Result<(),WpsError>{for effect in &mut self.effects{*effect=None;}self.queued=0;if self.peer.is_some(){let mut message=self.response(M::Nack)?;message.attribute(A::RegistrarNonce,&self.nonce.0)?;message.attribute(A::ConfigError,&18u16.to_be_bytes())?;self.send(3,message,false)?;}self.finish(RegistrarOutcome::Failed(error))}
    fn finish(&mut self,outcome:RegistrarOutcome)->Result<(),WpsError>{self.state=if matches!(outcome,RegistrarOutcome::Registered{..}){RegistrarState::Finished}else{RegistrarState::Failed};self.outcome=Some(outcome);self.clear();self.advertise(false)?;self.push(RegistrarEffect::CancelTimer)?;if let Some(peer)=self.peer{self.push(RegistrarEffect::Disconnect{peer})?;}Ok(())}
    fn clear(&mut self){self.private=None;self.keys=None;self.entropy.fill(0);self.secret1=SecretNonce::from_bytes([0;16]);self.secret2=SecretNonce::from_bytes([0;16]);for iv in &mut self.ivs{iv.0.fill(0);}self.last.clear();self.last_response.clear();self.last_request=None;self.incoming.clear();self.incoming_total=None;self.outgoing.clear();}
}
impl Drop for Registrar {fn drop(&mut self){self.clear();}}
fn security_flags(security:WpsSecurity)->(u16,u16){match security{WpsSecurity::Open=>(1,1),WpsSecurity::Wep=>(1,2),WpsSecurity::Wpa=>(2,4),WpsSecurity::Wpa2=>(0x20,8)}}
