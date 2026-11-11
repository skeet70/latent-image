use super::*;
use super::super::{WpsEnrollee,WpsConfiguration,WpsMethod,WpsEffect,WpsOutcome,WpsState};

fn ready_registrar(method:WpsMethod)->Registrar{
    let credentials=RegistrarCredentials::new(Ssid::parse(b"AP synthetic fixture").unwrap(),WpsSecurity::Wpa2,b"registrar passphrase",0).unwrap();
    let mut registrar=Registrar::new(MacAddress([2,0,0,0,0,1]),credentials);
    match method{WpsMethod::Pbc=>registrar.start_pbc(None).unwrap(),WpsMethod::Pin(pin)=>registrar.start_pin(pin,None).unwrap(),WpsMethod::GeneratePin=>panic!("explicit registrar PIN required")}
    for block in 0..9{assert!(matches!(registrar.poll(),Some(RegistrarEffect::Entropy)));registrar.supply_entropy([block+1;32]).unwrap();}
    let Some(RegistrarEffect::SelectedRegistrar{elements})=registrar.poll()else{panic!("missing advertisement")};assert!(!elements.as_bytes().is_empty());assert!(matches!(registrar.poll(),Some(RegistrarEffect::ArmTimer{milliseconds:120_000})));registrar
}
fn run_exchange(method:WpsMethod,enrollee_method:WpsMethod)->(Option<RegistrarOutcome>,Option<WpsOutcome>){
    let ap=MacAddress([2,0,0,0,0,1]);let station=MacAddress([2,0,0,0,0,2]);let mut registrar=ready_registrar(method);
    let mut enrollee=WpsEnrollee::new(station,WpsConfiguration{method:enrollee_method,bssid:Some(ap)}).unwrap();enrollee.start().unwrap();
    for block in 0..9{assert!(matches!(enrollee.poll(),Some(WpsEffect::Entropy)));enrollee.supply_entropy([block+11;32]).unwrap();}
    while enrollee.poll().is_some(){}enrollee.associate(ap).unwrap();let mut registered=None;let mut enrolled=None;
    for _ in 0..40{
        let mut delivered=false;
        while let Some(effect)=enrollee.poll(){if let WpsEffect::Transmit{frame,..}=effect{delivered=true;let _=registrar.receive_eapol(station,frame.as_bytes());}}
        while let Some(effect)=registrar.poll(){if let RegistrarEffect::Transmit{frame,..}=effect{delivered=true;if !matches!(enrollee.state(),WpsState::Finished|WpsState::Failed){let _=enrollee.receive_eapol(ap,frame.as_bytes());}}}
        registered=registrar.take_outcome().or(registered);enrolled=enrollee.take_outcome().or(enrolled);
        if !delivered{break;}
    }
    (registered,enrolled)
}
#[test]
fn pin_and_pbc_provision_actual_station_credentials(){
    for method in [WpsMethod::Pbc,WpsMethod::Pin(WpsPin::parse(b"12345670").unwrap())]{
        let (registered,enrolled)=run_exchange(method,method);assert_eq!(registered,Some(RegistrarOutcome::Registered{peer:MacAddress([2,0,0,0,0,2])}));
        let Some(WpsOutcome::Credentials(profile))=enrolled else{panic!("registration did not produce credentials")};assert_eq!(profile.ssid.as_bytes(),b"AP synthetic fixture");
        let expected=StationProfile::parse(profile.ssid,3,b"registrar passphrase",0).unwrap();assert_eq!(profile,expected);
    }
}
#[test]
fn wrong_pin_cannot_complete_or_install_credentials(){
    let (registered,enrolled)=run_exchange(WpsMethod::Pin(WpsPin::parse(b"1234").unwrap()),WpsMethod::Pin(WpsPin::parse(b"5678").unwrap()));
    assert!(!matches!(registered,Some(RegistrarOutcome::Registered{..})));assert!(matches!(enrolled,Some(WpsOutcome::Failed(WpsError::Authentication))));
}
#[test]
fn cancellation_removes_selected_registrar_and_timeout_never_succeeds(){
    let mut registrar=ready_registrar(WpsMethod::Pbc);assert!(registrar.is_drained());registrar.cancel();assert!(!registrar.is_drained());assert_eq!(registrar.take_outcome(),Some(RegistrarOutcome::Cancelled));
    let Some(RegistrarEffect::SelectedRegistrar{elements})=registrar.poll()else{panic!("missing disabled advertisement")};let attrs=Attributes::parse(&elements.as_bytes()[6..]).unwrap();assert_eq!(attrs.get(A::SelectedRegistrar).unwrap(),&[0]);assert!(matches!(registrar.poll(),Some(RegistrarEffect::CancelTimer)));
    assert!(registrar.is_drained());
    registrar.start_pbc(None).unwrap();for block in 0..9{assert!(matches!(registrar.poll(),Some(RegistrarEffect::Entropy)));registrar.supply_entropy([block+1;32]).unwrap();}while registrar.poll().is_some(){}registrar.timeout().unwrap();assert_eq!(registrar.take_outcome(),Some(RegistrarOutcome::Failed(WpsError::Timeout)));
}
#[test]
fn retransmission_reuses_the_same_eap_identifier_and_bytes(){
    let mut registrar=ready_registrar(WpsMethod::Pbc);let peer=MacAddress([2;6]);registrar.receive_eapol(peer,&[1,1,0,0]).unwrap();let Some(RegistrarEffect::Transmit{frame:first,..})=registrar.poll()else{panic!("missing identity request")};
    registrar.retry().unwrap();let Some(RegistrarEffect::Transmit{frame:retry,..})=registrar.poll()else{panic!("missing retry")};assert_eq!(first.as_bytes(),retry.as_bytes());
    registrar.receive_eapol(MacAddress([3;6]),&[1,1,0,0]).unwrap_err();assert_eq!(registrar.take_outcome(),Some(RegistrarOutcome::Failed(WpsError::Overlap)));
}

#[test]
fn initial_beacon_is_configured_and_locked_without_selecting_a_registrar(){
    let credentials=RegistrarCredentials::new(Ssid::parse(b"fixture").unwrap(),WpsSecurity::Wpa2,b"fixture password",0).unwrap();let registrar=Registrar::new(MacAddress([2;6]),credentials);
    let elements=registrar.initial_advertisement().unwrap();let attrs=Attributes::parse(&elements.as_bytes()[6..]).unwrap();
    assert_eq!(attrs.get(A::WpsState).unwrap(),&[2]);assert_eq!(attrs.get(A::ApSetupLocked).unwrap(),&[1]);assert_eq!(attrs.get(A::SelectedRegistrar).unwrap(),&[0]);
}

#[test]
fn pin_authorization_outlives_selected_registrar_advertisement(){
    let mut registrar=ready_registrar(WpsMethod::Pin(WpsPin::parse(b"1234").unwrap()));registrar.timeout().unwrap();
    assert_eq!(registrar.take_outcome(),None);assert_eq!(registrar.state(),RegistrarState::Waiting);
    let Some(RegistrarEffect::SelectedRegistrar{elements})=registrar.poll()else{panic!("missing disabled advertisement")};assert_eq!(Attributes::parse(&elements.as_bytes()[6..]).unwrap().get(A::SelectedRegistrar).unwrap(),&[0]);
    registrar.receive_eapol(MacAddress([2;6]),&[1,1,0,0]).unwrap();assert!(matches!(registrar.poll(),Some(RegistrarEffect::Transmit{..})));
}
