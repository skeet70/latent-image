use super::*;
use crate::af_capture_actor::LensMachineState;
use crate::lens_queue_runtime::physical_fixture::{expire, initialize, received, setup};

fn owner(value: u32) -> CallbackOwner { CallbackOwner::new(value).unwrap() }
fn consumer() -> LensRequestConsumer { LensRequestConsumer::from_machine_snapshot(LensMachineState::from_snapshot(2).unwrap()) }

#[test]
fn registration_only_roots_neither_submit_nor_complete_and_share_busy() {
    for address in [0xefc2f8, 0xefc360, 0xefc3a4, 0xefc3e8, 0xefc42c, 0xefc470, 0xefc4b4, 0xefc4f8] {
        let (mut lens, physical) = setup();
        let mut encoded = [0u8; 0x100];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
        let mut selected = consumer();
        let mut application = AfRuntime::new();
        let root = RegistrationRoot::from_address(address).unwrap();
        let Progress::Waiting(id) = application.begin(Request::Register(root), Some(owner(1)), &mut selected, &mut lens, &mut configuration).unwrap() else { panic!("registration did not wait"); };
        assert!(physical.words.borrow().is_empty());
        assert_eq!(lens.pop_publication(), None);
        application.detach_waiter(id).unwrap();
        let Progress::Callback(completion) = application.begin(Request::ReadPosition, Some(owner(2)), &mut selected, &mut lens, &mut configuration).unwrap() else { panic!("busy callback not delivered"); };
        assert_eq!(completion.owner, owner(2));
        assert_eq!(completion.status.abi(), -95);
        assert!(application.busy());
        assert!(physical.words.borrow().is_empty());
        assert_eq!(application.delivered(id), Err(LifetimeError::DeliveryPending));
    }
}

#[test]
fn parameter_callbacks_are_synchronous_optional_and_do_not_claim_busy() {
    let (mut lens, physical) = setup();
    let mut encoded = [0u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut selected = consumer();
    let mut application = AfRuntime::new();
    application.begin(Request::Register(RegistrationRoot::Initialize), Some(owner(1)), &mut selected, &mut lens, &mut configuration).unwrap();
    for request in [Request::PrimaryParameter, Request::SecondaryParameter] {
        assert_eq!(application.begin(request, None, &mut selected, &mut lens, &mut configuration).unwrap(), Progress::NoCallback);
        assert_eq!(application.begin(request, Some(owner(2)), &mut selected, &mut lens, &mut configuration).unwrap(), Progress::Callback(Completion { owner: owner(2), status: CallbackStatus::Success, registration: None, waiter_attached: true }));
    }
    assert!(application.busy());
    assert!(physical.words.borrow().is_empty());
}

#[test]
fn position_response_updates_configuration_before_callback_and_clears_busy_after_delivery() {
    let (mut lens, physical) = setup();
    initialize(&mut lens);
    let mut encoded = [0u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut selected = consumer();
    let mut application = AfRuntime::new();
    let Progress::Waiting(id) = application.begin(Request::ReadPosition, Some(owner(1)), &mut selected, &mut lens, &mut configuration).unwrap() else { panic!("read did not wait"); };
    received(&mut lens, &physical, &[0, 0, 0, 0]);
    expire(&mut lens, &physical);
    received(&mut lens, &physical, &[0x08, 0, 0xc8, 0x3a]);
    expire(&mut lens, &physical);
    received(&mut lens, &physical, &[0x92, 0x34, 0x48, 0x10]);
    assert_eq!(lens.pop_publication(), None);
    expire(&mut lens, &physical);
    received(&mut lens, &physical, &[0x08, 0, 0x80, 0x12]);
    let publication = lens.pop_publication().unwrap();
    let completion = application.publication(publication, &mut selected, &mut configuration).unwrap().unwrap();
    assert_eq!(completion.status, CallbackStatus::Success);
    assert_eq!(configuration.byte(0x82), Some(0x12));
    assert_eq!(configuration.byte(0x83), Some(0x34));
    assert!(application.busy());
    application.delivered(id).unwrap();
    assert!(!application.busy());
    assert_eq!(application.delivered(id), Err(LifetimeError::Stale));
    assert_eq!(application.publication(publication, &mut selected, &mut configuration).unwrap(), None);
}

#[test]
fn cancelled_wire_still_runs_actual_application_callback_without_resurrecting_waiter() {
    let (mut lens, _) = setup();
    initialize(&mut lens);
    let mut encoded = [0xa5u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut selected = consumer();
    let mut application = AfRuntime::new();
    let Progress::Waiting(id) = application.begin(Request::ReadPosition, Some(owner(1)), &mut selected, &mut lens, &mut configuration).unwrap() else { panic!("read did not wait"); };
    application.detach_waiter(id).unwrap();
    lens.cancel().unwrap();
    let completion = application.publication(lens.pop_publication().unwrap(), &mut selected, &mut configuration).unwrap().unwrap();
    assert_eq!(completion.status, CallbackStatus::Success);
    assert!(!completion.waiter_attached);
    assert_eq!(configuration.byte(0x82), Some(0));
    assert_eq!(configuration.byte(0x83), Some(0));
    application.delivered(id).unwrap();
    let Progress::Waiting(next) = application.begin(Request::Register(RegistrationRoot::Select), Some(owner(2)), &mut selected, &mut lens, &mut configuration).unwrap() else { panic!("new registration did not wait"); };
    assert_eq!(application.detach_waiter(id), Err(LifetimeError::Stale));
    assert_ne!(next, id);
}
