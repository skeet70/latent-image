use super::*;

#[test]
fn relative_focus_targets_saturate_at_both_signed_limits() {
    let mut context = MotionContext::initialized(ZoomPosition(0));
    context.current_focus = FocusPosition(32000);
    assert_eq!(context.focus_target(FocusDelta(1000)), FocusPosition(32767));
    context.current_focus = FocusPosition(-32000);
    assert_eq!(context.focus_target(FocusDelta(-1000)), FocusPosition(-32768));
    context.current_focus = FocusPosition(500);
    assert_eq!(context.focus_target(FocusDelta(-700)), FocusPosition(-200));
}

#[test]
fn missing_axis_status_takes_precedence_over_in_progress() {
    let mut context = MotionContext::initialized(ZoomPosition(20));
    assert_eq!(context.focus_status().abi(), 0);
    context.focus_in_progress = true;
    assert_eq!(context.focus_status().abi(), 1);
    context.missing_focus = true;
    assert_eq!(context.focus_status().abi(), 2);
    context.iris_in_progress = true;
    assert_eq!(context.iris_status().abi(), 1);
    context.missing_iris = true;
    assert_eq!(context.iris_status().abi(), 2);
    context.zoom_in_progress = true;
    context.missing_zoom = true;
    assert_eq!(context.zoom_status().abi(), 2);
}

#[test]
fn application_position_queries_publish_only_their_source_failure_effect() {
    let mut encoded = [0xa5u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut context = MotionContext::initialized(ZoomPosition(0));
    context.current_focus = FocusPosition(-1234);
    context.current_zoom = ZoomPosition(160);
    assert_eq!(context.application_focus(&mut configuration).unwrap(), FocusPosition(-1234));
    assert_eq!(configuration.byte(0x81), Some(0xa5));
    assert_eq!(context.application_zoom(), 160);
    context.missing_focus = true;
    context.missing_zoom = true;
    assert_eq!(context.application_focus(&mut configuration).unwrap(), FocusPosition(0));
    assert_eq!(configuration.byte(0x81), Some(1));
    assert_eq!(configuration.byte(0x82), Some(0xa5));
    assert_eq!(context.application_zoom(), 0x80);
}

#[test]
fn failure_output_selection_uses_exact_service_command_classes() {
    for (command, offset) in [(0x56, 0x81), (0x61, 0x81), (0x62, 0x81), (0x71, 0x81), (0x60, 0x86), (0x6b, 0x82), (0x7b, 0x82), (0x70, 0x83)] {
        let mut encoded = [0u8; 0x100];
        let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
        ServiceOperation::from_abi(command).report_failure(&mut configuration).unwrap();
        for key in [0x81, 0x82, 0x83, 0x86] { assert_eq!(configuration.byte(key), Some(u8::from(key == offset))); }
    }
    let mut encoded = [0u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    ServiceOperation::from_abi(0x64).report_failure(&mut configuration).unwrap();
    assert_eq!(configuration.byte(0x81), Some(0));
    assert_eq!(configuration.byte(0x86), Some(0));
}

#[test]
fn focus_home_and_movement_report_missing_state_to_distinct_outputs() {
    let mut encoded = [0u8; 0x100];
    let mut configuration = ConfigImages::adopt(&[], &[], &[], &mut encoded, &[]);
    let mut context = MotionContext::initialized(ZoomPosition(0));
    context.missing_focus = true;
    context.service = Some(ServiceOperation::Request61);
    context.report_focus_completion(false, &mut configuration).unwrap();
    assert_eq!(configuration.byte(0x81), Some(0));
    context.report_focus_completion(true, &mut configuration).unwrap();
    assert_eq!(configuration.byte(0x81), Some(1));
    context.service = Some(ServiceOperation::Request60);
    context.report_focus_completion(false, &mut configuration).unwrap();
    assert_eq!(configuration.byte(0x86), Some(1));
}

#[test]
fn history_sample_is_consumed_and_previous_zoom_is_updated() {
    let mut context = MotionContext::initialized(ZoomPosition(50));
    context.current_zoom = ZoomPosition(1234);
    context.previous_sample = ZoomPosition(99);
    assert_eq!(context.take_position_sample(), ZoomPosition(99));
    assert_eq!(context.position_sample, ZoomPosition(1234));
    assert_eq!(context.take_position_sample(), ZoomPosition(0));
    assert_eq!(context.zoom_reference, ZoomPosition(50));
}
