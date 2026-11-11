use super::{MiddlePropertyActions, PropertyProgress};
use crate::{backend::ConfigImages, configuration_actions_low::{ActionCompletion, CpuReturn}, configuration_actions_middle::{MiddleAction, MiddleArguments}, lens_queue_runtime::physical_fixture::{initialize, setup}};
use xm1_lens_transport::control_actor::ControlActor;
use xm1_lens_transport::control_types::SharedReadiness;

#[test]
fn stopped_option_read_sets_caller_error_and_preserves_success_notification() {
    let (mut runtime, physical) = setup();
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut actor = MiddlePropertyActions::default();
    let mut bytes = [0xa5; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let arguments = MiddleArguments::from_abi(&[0x81, 0, 0x82, 0, 0x83, 0, 0x84, 0]);
    assert_eq!(actor.begin(MiddleAction::parse(64).unwrap(), arguments, &mut images, &device, &mut runtime).unwrap(), PropertyProgress::Waiting);
    let publication = runtime.pop_publication().unwrap();
    let Some(PropertyProgress::Completed(completion)) = actor.publication(publication, &mut images, &device, &mut runtime).unwrap() else { panic!() };
    assert_eq!(completion.return_code, CpuReturn::REJECTED);
    assert_eq!(completion.notification, ActionCompletion::service_success().notification);
    assert_eq!(images.byte(0x82), Some(0));
    assert_eq!(images.byte(0x83), Some(0));
    assert_eq!(images.byte(0x84), Some(1));
    assert!(!physical.requested.get());
    assert!(!actor.is_waiting());
}

#[test]
fn cancellation_ignores_late_property_publication_and_keeps_caller_bytes() {
    let (mut runtime, _) = setup();
    initialize(&mut runtime);
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut actor = MiddlePropertyActions::default();
    let mut bytes = [0xa5; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let arguments = MiddleArguments::from_abi(&[0x81, 0, 0x82, 0, 0x83, 0, 0x84, 0]);
    actor.begin(MiddleAction::parse(74).unwrap(), arguments, &mut images, &device, &mut runtime).unwrap();
    actor.cancel(&mut runtime).unwrap();
    let late = runtime.pop_publication().unwrap();
    assert_eq!(actor.publication(late, &mut images, &device, &mut runtime).unwrap(), None);
    assert_eq!(images.byte(0x82), Some(0xa5));
    assert_eq!(images.byte(0x83), Some(0xa5));
    assert_eq!(images.byte(0x84), Some(0xa5));
}

#[test]
fn unavailable_device_clears_banks_without_starting_a_property_command() {
    let (mut runtime, _) = setup();
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut actor = MiddlePropertyActions::default();
    let mut bytes = [0xa5; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let result = actor.begin(MiddleAction::parse(66).unwrap(), MiddleArguments::from_abi(&[]), &mut images, &device, &mut runtime).unwrap();
    assert_eq!(result, PropertyProgress::Completed(ActionCompletion::service_success()));
    assert!((0x83..=0x8a).all(|key| images.byte(key) == Some(0)));
    assert_eq!(runtime.pop_publication(), None);
}

#[test]
fn malformed_option_arguments_do_not_start_a_wire_transaction() {
    let (mut runtime, _) = setup();
    let readiness = SharedReadiness::new();
    let device = ControlActor::new([0; 128], false, &readiness);
    let mut actor = MiddlePropertyActions::default();
    let mut bytes = [0; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    assert!(actor.begin(MiddleAction::parse(64).unwrap(), MiddleArguments::from_abi(&[0x81, 0]), &mut images, &device, &mut runtime).is_err());
    assert_eq!(runtime.pop_publication(), None);
    assert!(!actor.is_waiting());
}
