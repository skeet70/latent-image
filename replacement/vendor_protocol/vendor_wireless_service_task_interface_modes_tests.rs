use super::*;

#[test]
fn control_transitions_publish_remote_property_values_in_order() {
    let mut control = RemoteControl::new();
    control.initialize(InterfaceMode::Mode2);
    control.select(RemoteSelection::Configure);
    control.begin_configure();
    control.select(RemoteSelection::Reset);
    control.begin_reset();
    let properties: Vec<_> = control.take_properties().entries().map(|entry| (entry.code.wire(), entry.value)).collect();
    assert_eq!(properties, [(0xdf00, 1), (0xd220, 1), (0xdf00, 17), (0xd52f, 1)]);
    assert!(control.take_properties().entries().next().is_none());
    assert!(control.configuring());
    assert!(control.resetting());
}

#[test]
fn stopping_does_not_discard_already_published_property_changes() {
    let mut control = RemoteControl::new();
    control.select(RemoteSelection::Transfer);
    control.publish_count(23);
    control.stop();
    assert_eq!(control.selected(), 0);
    assert_eq!(control.object_count(), 23);
    let properties: Vec<_> = control.take_properties().entries().map(|entry| (entry.code.wire(), entry.value)).collect();
    assert_eq!(properties, [(0xdf00, 3), (0xd222, 23)]);
}

#[test]
fn cancelling_configure_clears_value_without_publishing_an_extra_change() {
    let mut control = RemoteControl::new();
    control.begin_configure();
    control.cancel_configure();
    assert!(!control.configuring());
    let properties: Vec<_> = control.take_properties().entries().collect();
    assert_eq!(properties, [RemoteProperty { code: RemotePropertyCode::Configure, value: 1 }]);
}

#[test]
fn property_change_capacity_preserves_first_eight_changes_and_current_value() {
    let mut control = RemoteControl::new();
    for count in 1..=10 { control.publish_count(count); }
    assert_eq!(control.object_count(), 10);
    let values: Vec<_> = control.take_properties().entries().map(|entry| entry.value).collect();
    assert_eq!(values, [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn timestamp_conversion_discards_separators_and_prefixes_century() {
    let stamp = ObjectTimestamp::parse(b"26/11/10 12:34:56").unwrap();
    assert_eq!(stamp.bytes(), b"20261110123456\0\0");
    assert_eq!(ObjectTimestamp::parse(b"26/11/10"), Err(InterfaceError::InvalidTimestamp));
}

#[test]
fn names_reject_unrepresentable_terminated_strings() {
    assert_eq!(ObjectName::parse(b"A\0B"), Err(InterfaceError::InvalidName));
    assert_eq!(ObjectName::parse(&[b'A'; 48]), Err(InterfaceError::InvalidName));
    assert_eq!(ObjectName::parse(b"DSCF0001.JPG").unwrap().bytes(), b"DSCF0001.JPG");
}

#[test]
fn configuration_header_requires_marker_and_a_terminated_name_at_offset_seventeen() {
    let mut header = [0; 40];
    header[0] = 0x5a;
    header[17..29].copy_from_slice(b"DSCF0001.JPG");
    assert_eq!(ObjectName::from_header(&header).unwrap().bytes(), b"DSCF0001.JPG");
    header[0] = 0;
    assert_eq!(ObjectName::from_header(&header), Err(InterfaceError::InvalidHeader));
    header[0] = 0x5a;
    assert_eq!(ObjectName::from_header(&header[..29]), Err(InterfaceError::InvalidName));
}

#[test]
fn interface_capabilities_preserve_null_operation_slots() {
    assert!(!InterfaceMode::Mode0.supports(InterfaceSlot::Reset));
    assert!(!InterfaceMode::Mode0.supports(InterfaceSlot::CancelReset));
    for mode in [InterfaceMode::Mode1, InterfaceMode::Mode2, InterfaceMode::Mode5] { assert!(!mode.supports(InterfaceSlot::Start)); }
    assert!(InterfaceMode::Mode3.supports(InterfaceSlot::Start));
    assert!(InterfaceMode::Mode3.supports(InterfaceSlot::Reset));
}
