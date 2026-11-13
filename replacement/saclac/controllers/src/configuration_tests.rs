use crate::statistics::{Changes, Configuration, Group, Outcome};
use crate::stock;

#[test]
fn combined_autofocus_update_commits_measurement_then_processing_then_geometry() {
    let mut configuration = Configuration::new();
    assert_eq!(configuration.start(Changes { processing: true, geometry: true, measurement: true }), Outcome::Configure(Group::Measurement));
    assert_eq!(configuration.complete(Group::Measurement), Outcome::Configure(Group::Processing));
    assert_eq!(configuration.complete(Group::Processing), Outcome::Configure(Group::Geometry));
    assert_eq!(configuration.complete(Group::Geometry), Outcome::Complete);
}

#[test]
fn processing_and_geometry_begin_with_geometry() {
    let mut configuration = Configuration::new();
    assert_eq!(configuration.start(Changes { processing: true, geometry: true, measurement: false }), Outcome::Configure(Group::Geometry));
    assert_eq!(configuration.complete(Group::Geometry), Outcome::Configure(Group::Processing));
    assert_eq!(configuration.complete(Group::Processing), Outcome::Complete);
}

#[test]
fn unchanged_configuration_needs_no_peer_completion() {
    let mut configuration = Configuration::new();
    assert_eq!(configuration.start(Changes::default()), Outcome::Complete);
    assert_eq!(configuration.start(Changes { processing: false, geometry: false, measurement: true }), Outcome::Configure(Group::Measurement));
    assert_eq!(configuration.complete(Group::Measurement), Outcome::Complete);
}

#[test]
fn unrelated_completion_and_second_start_do_not_replace_current_update() {
    let mut configuration = Configuration::new();
    assert_eq!(configuration.start(Changes { processing: true, geometry: false, measurement: false }), Outcome::Configure(Group::Processing));
    assert_eq!(configuration.complete(Group::Geometry), Outcome::UnexpectedCompletion);
    assert_eq!(configuration.start(Changes { processing: false, geometry: false, measurement: true }), Outcome::Busy);
    assert_eq!(configuration.complete(Group::Processing), Outcome::Complete);
}

#[test]
fn live_and_capture_use_different_geometry_flags() {
    let request = stock::RecordFlags::new(1 << 6, 0);
    let capture = stock::RecordFlags::new(0, 0);
    assert_eq!(stock::autofocus_changes(request, capture, true), Changes { processing: false, geometry: true, measurement: false });
    assert_eq!(stock::autofocus_changes(request, capture, false), Changes::default());
    let request = stock::RecordFlags::new(1 << 7, 0);
    assert_eq!(stock::autofocus_changes(request, capture, true), Changes::default());
    assert_eq!(stock::autofocus_changes(request, capture, false), Changes { processing: false, geometry: true, measurement: false });
}

#[test]
fn capture_only_measurement_features_do_not_reconfigure_live_measurement() {
    let request = stock::RecordFlags::new(1 << 1, 0);
    let capture = stock::RecordFlags::new(0, 0);
    assert_eq!(stock::autofocus_changes(request, capture, true), Changes::default());
    assert_eq!(stock::autofocus_changes(request, capture, false), Changes { processing: false, geometry: false, measurement: true });
}

#[test]
fn configuration_wire_operations_and_completions_pair_at_boundary() {
    assert_eq!(stock::configuration_operation(Group::Processing), 0x4012);
    assert_eq!(stock::configuration_operation(Group::Geometry), 0x4010);
    assert_eq!(stock::configuration_operation(Group::Measurement), 0x402c);
    assert_eq!(stock::configuration_completed(0x73), Some(Group::Processing));
    assert_eq!(stock::configuration_completed(0x71), Some(Group::Geometry));
    assert_eq!(stock::configuration_completed(0x80), Some(Group::Measurement));
    assert_eq!(stock::configuration_completed(0x72), None);
}
