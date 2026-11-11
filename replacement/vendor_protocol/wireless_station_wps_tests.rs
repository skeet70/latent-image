use super::*;

fn request(id:u8,opcode:u8,payload:&[u8])->WpsFrame {
    let length=14+payload.len();let mut result=WpsFrame::new();result.append(&[1,0]).unwrap();result.append(&(length as u16).to_be_bytes()).unwrap();result.append(&[1,id]).unwrap();result.append(&(length as u16).to_be_bytes()).unwrap();result.append(&[254,0,0x37,0x2a,0,0,0,1,opcode,0]).unwrap();result.append(payload).unwrap();result
}
fn transmitted(engine:&mut WpsEnrollee)->WpsFrame {
    while let Some(effect)=engine.poll(){if let WpsEffect::Transmit{frame,..}=effect{return frame;}}
    panic!("expected EAPOL transmission")
}
fn body(frame:&WpsFrame)->&[u8] {&frame.as_bytes()[18..]}
fn authenticated(keys:&WpsKeys,previous:&[u8],mut message:WpsFrame)->WpsFrame {
    let auth=keys.authenticator(&[previous,message.as_bytes()]);message.attribute(A::Authenticator,&auth.0).unwrap();message
}
fn encrypted(keys:&WpsKeys,iv:u8,settings:&WpsFrame)->WpsFrame {
    let mut bytes=[0;2048];let length=keys.encrypt_settings(&Iv([iv;16]),settings.as_bytes(),&mut bytes).unwrap();let mut result=WpsFrame::new();result.append(&bytes[..length]).unwrap();result
}

#[test]
fn authenticated_registration_delivers_credentials_only_after_m8() {
    let local=MacAddress([2,0,0,0,0,1]);let ap=MacAddress([2,0,0,0,0,2]);let pin=WpsPin::parse(b"12345670").unwrap();
    let mut engine=WpsEnrollee::new(local,WpsConfiguration{method:WpsMethod::Pin(pin),bssid:Some(ap)}).unwrap();engine.start().unwrap();
    for block in 0..9 {assert!(matches!(engine.poll(),Some(WpsEffect::Entropy)));engine.supply_entropy([block+1;32]).unwrap();}
    while engine.poll().is_some(){}engine.associate(ap).unwrap();assert_eq!(transmitted(&mut engine).as_bytes(),&[1,1,0,0]);
    engine.receive_eapol(ap,request(1,1,&[]).as_bytes()).unwrap();let m1=transmitted(&mut engine);let attrs=Attributes::parse(body(&m1)).unwrap();
    assert_eq!(M::parse(attrs.get(A::MessageType).unwrap()).unwrap(),M::M1);assert_eq!(attrs.get(A::Mac).unwrap(),&local.0);
    let enrollee_public=DhPublic::from_bytes(attrs.fixed(A::PublicKey).unwrap()).unwrap();let enrollee_nonce=Nonce(attrs.fixed(A::EnrolleeNonce).unwrap());let registrar_nonce=Nonce([19;16]);
    let mut exponent=[0;192];exponent[191]=23;let registrar_private=DhPrivate::from_bytes(exponent).unwrap();let registrar_public=registrar_private.public_key();
    let keys=registrar_private.derive_keys(&enrollee_public,&enrollee_nonce,&EnrolleeMac(local.0),&registrar_nonce).unwrap();
    let mut m2=WpsFrame::message(M::M2).unwrap();m2.attribute(A::EnrolleeNonce,&enrollee_nonce.0).unwrap();m2.attribute(A::RegistrarNonce,&registrar_nonce.0).unwrap();m2.attribute(A::UuidR,&[20;16]).unwrap();m2.attribute(A::PublicKey,registrar_public.as_bytes()).unwrap();let m2=authenticated(&keys,body(&m1),m2);
    engine.receive_eapol(ap,request(2,4,m2.as_bytes()).as_bytes()).unwrap();let m3=transmitted(&mut engine);assert!(engine.take_outcome().is_none());
    let attrs=Attributes::parse(body(&m3)).unwrap();let (unsigned,auth)=attrs.authenticated().unwrap();assert!(keys.verify_authenticator(&[m2.as_bytes(),unsigned],&Authenticator(auth)));
    let proofs=keys.password_proofs(pin.as_bytes()).unwrap();let r1=SecretNonce::from_bytes([31;16]);let r2=SecretNonce::from_bytes([32;16]);
    let h1=proofs.commitment(&r1,&enrollee_public,&registrar_public,PasswordHalf::First);let h2=proofs.commitment(&r2,&enrollee_public,&registrar_public,PasswordHalf::Second);
    let mut plain=WpsFrame::new();plain.attribute(A::RSnonce1,r1.as_bytes()).unwrap();let cipher=encrypted(&keys,41,&plain);
    let mut m4=WpsFrame::message(M::M4).unwrap();m4.attribute(A::EnrolleeNonce,&enrollee_nonce.0).unwrap();m4.attribute(A::RHash1,&h1.0).unwrap();m4.attribute(A::RHash2,&h2.0).unwrap();m4.attribute(A::EncryptedSettings,cipher.as_bytes()).unwrap();let m4=authenticated(&keys,body(&m3),m4);
    engine.receive_eapol(ap,request(3,4,m4.as_bytes()).as_bytes()).unwrap();let m5=transmitted(&mut engine);assert!(engine.take_outcome().is_none());
    let attrs5=Attributes::parse(body(&m5)).unwrap();let mut decoded=[0;2048];let length=keys.decrypt_settings(attrs5.get(A::EncryptedSettings).unwrap(),&mut decoded).unwrap();let es1=Attributes::parse(&decoded[..length]).unwrap().fixed(A::ESnonce1).unwrap();
    assert!(proofs.verify_commitment(&SecretNonce::from_bytes(es1),&enrollee_public,&registrar_public,PasswordHalf::First,&Commitment(attrs.fixed(A::EHash1).unwrap())));
    let mut plain=WpsFrame::new();plain.attribute(A::RSnonce2,r2.as_bytes()).unwrap();let cipher=encrypted(&keys,42,&plain);let mut m6=WpsFrame::message(M::M6).unwrap();m6.attribute(A::EnrolleeNonce,&enrollee_nonce.0).unwrap();m6.attribute(A::EncryptedSettings,cipher.as_bytes()).unwrap();let m6=authenticated(&keys,body(&m5),m6);
    engine.receive_eapol(ap,request(4,4,m6.as_bytes()).as_bytes()).unwrap();let m7=transmitted(&mut engine);assert!(engine.take_outcome().is_none());
    let mut credential=WpsFrame::new();credential.attribute(A::Ssid,b"registrar fixture").unwrap();credential.attribute(A::AuthType,&[0,0x20]).unwrap();credential.attribute(A::Encryption,&[0,8]).unwrap();credential.attribute(A::Mac,&local.0).unwrap();credential.attribute(A::NetworkKey,b"fixture password").unwrap();let mut plain=WpsFrame::new();plain.attribute(A::Credential,credential.as_bytes()).unwrap();let cipher=encrypted(&keys,43,&plain);
    let mut m8=WpsFrame::message(M::M8).unwrap();m8.attribute(A::EnrolleeNonce,&enrollee_nonce.0).unwrap();m8.attribute(A::EncryptedSettings,cipher.as_bytes()).unwrap();let m8=authenticated(&keys,body(&m7),m8);
    engine.receive_eapol(ap,request(5,4,m8.as_bytes()).as_bytes()).unwrap();let done=transmitted(&mut engine);assert_eq!(done.as_bytes()[16],5);
    let Some(WpsOutcome::Credentials(profile))=engine.take_outcome() else{panic!("missing credentials")};assert_eq!(profile.ssid.as_bytes(),b"registrar fixture");assert!(matches!(profile.credentials,super::super::Credentials::Personal(_)));
    assert!(matches!(engine.poll(),Some(WpsEffect::CancelTimer)));assert!(matches!(engine.poll(),Some(WpsEffect::Disconnect)));
}

#[test]
fn scan_concatenates_vendor_fragments_and_distinguishes_pbc_from_pin() {
    let local=MacAddress([2;6]);let ap=MacAddress([3;6]);
    let pbc=WpsEnrollee::new(local,WpsConfiguration{method:WpsMethod::Pbc,bssid:None}).unwrap();
    let pin=WpsEnrollee::new(local,WpsConfiguration{method:WpsMethod::Pin(WpsPin::parse(b"1234").unwrap()),bssid:None}).unwrap();
    let elements=[221,7,0,0x50,0xf2,4,0x10,0x41,0,221,12,0,0x50,0xf2,4,1,1,0x10,0x12,0,2,0,4];
    assert_eq!(pbc.scan_eligible(ap,&elements),Ok(true));assert_eq!(pin.scan_eligible(ap,&elements),Ok(false));
    assert_eq!(pbc.scan_eligible(ap,&elements[..elements.len()-1]),Err(WpsError::Truncated));
}

#[test]
fn repeated_eap_request_replays_identical_identity_response() {
    let ap=MacAddress([3;6]);let mut engine=WpsEnrollee::new(MacAddress([2;6]),WpsConfiguration{method:WpsMethod::Pbc,bssid:None}).unwrap();
    engine.start().unwrap();for block in 0..9{assert!(matches!(engine.poll(),Some(WpsEffect::Entropy)));engine.supply_entropy([block+1;32]).unwrap();}
    while engine.poll().is_some(){}engine.associate(ap).unwrap();let _=transmitted(&mut engine);
    let identity=[1,0,0,5,1,17,0,5,1];engine.receive_eapol(ap,&identity).unwrap();let first=transmitted(&mut engine);
    engine.receive_eapol(ap,&identity).unwrap();assert_eq!(transmitted(&mut engine).as_bytes(),first.as_bytes());assert!(engine.take_outcome().is_none());
    engine.timeout().unwrap();assert_eq!(engine.take_outcome(),Some(WpsOutcome::Failed(WpsError::Timeout)));
}
