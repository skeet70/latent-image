use super::{CallbackDisposition, MiddleAction, MiddleArguments, execute_empty, execute_immediate, publish_byte_pair};
use crate::{backend::ConfigImages, configuration_actions_low::{ActionCompletion, ConfigurationKey}};

#[test]
fn queue_identity_uses_wrapper_slot_one_lower_than_action() {
    assert_eq!(MiddleAction::parse(35).unwrap().wrapper(), 0xecc6fc);
    assert_eq!(MiddleAction::parse(35).unwrap().target(), 0xdbcd4c);
    assert_eq!(MiddleAction::parse(74).unwrap().wrapper(), 0xeccaa4);
    assert_eq!(MiddleAction::parse(74).unwrap().target(), 0xdbf764);
    assert!(MiddleAction::parse(34).is_err());
    assert!(MiddleAction::parse(75).is_err());
}

#[test]
fn empty_actions_finish_without_an_argument_buffer() {
    for action in [36, 39, 59, 60, 61, 62] {
        assert_eq!(execute_empty(MiddleAction::parse(action).unwrap()), Some(ActionCompletion::service_success()));
    }
    assert_eq!(execute_empty(MiddleAction::parse(35).unwrap()), None);
}

#[test]
fn immediate_record_callbacks_publish_success_to_caller_selected_key() {
    for action in [71, 73] {
        let mut bytes = [0xa5; 256];
        let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
        let result = execute_immediate(MiddleAction::parse(action).unwrap(), MiddleArguments::from_abi(&[0x81, 0, 0x82, 0]), &mut images).unwrap().unwrap();
        assert_eq!(result, ActionCompletion::service_success());
        assert_eq!(images.byte(0x81), Some(0xa5));
        assert_eq!(images.byte(0x82), Some(0));
    }
}



#[test]
fn failed_byte_pair_callback_leaves_configuration_unchanged() {
    let mut bytes = [0x5a; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    publish_byte_pair(CallbackDisposition::Failed, 0x1234, ConfigurationKey::from_abi(0x81), ConfigurationKey::from_abi(0x82), &mut images).unwrap();
    assert_eq!(images.byte(0x81), Some(0x5a));
    assert_eq!(images.byte(0x82), Some(0x5a));
    publish_byte_pair(CallbackDisposition::Accepted, 0x1234, ConfigurationKey::from_abi(0x81), ConfigurationKey::from_abi(0x82), &mut images).unwrap();
    assert_eq!(images.byte(0x81), Some(0x12));
    assert_eq!(images.byte(0x82), Some(0x34));
}


#[test]
fn drive_a_query_publishes_zero_without_accessing_nand() {
    let mut bytes = [0xa5; 256];
    let mut images = ConfigImages::adopt(&[], &[], &[], &mut bytes, &[]);
    let result = super::execute_immediate(MiddleAction::parse(38).unwrap(), MiddleArguments::from_abi(&[0x81, 0]), &mut images).unwrap().unwrap();
    assert_eq!(result, ActionCompletion::service_success());
    assert_eq!(images.byte(0x81), Some(0));
}
