use crate::{stock, Message};

#[test]
fn all_command_producers_parse_with_their_payloads() {
    for command in stock::COMMANDS {
        let payload = [0xa5; 168];
        let message = stock::parse(0x3b00 + u16::from(command.code), &payload[..command.payload_bytes]).unwrap();
        assert!(matches!(message, Message::Command(value) if value.kind == command.kind && value.parameters.bytes() == &payload[..command.payload_bytes]));
    }
}

#[test]
fn oversized_command_is_refused() {
    assert_eq!(stock::parse(0x3b01, &[0; 5]), Err(stock::ParseError::Oversize));
}

#[test]
fn callback_word_and_payload_survive_intake() {
    let payload = [0x34, 0x12, 0xcd, 0xab];
    let Message::Command(command) = stock::parse(0x3b01, &payload).unwrap() else { panic!("not a command") };
    assert_eq!(stock::callback_word(stock::client_callback(command.parameters).unwrap()), 0xabcd1234);
}

#[test]
fn each_callback_post_is_an_admitted_completion() {
    for callback in stock::CALLBACKS {
        for code in callback.posts {
            let Message::Completed(message) = stock::parse(0x3b00 + u16::from(*code), &[]).unwrap() else { panic!("not a completion") };
            assert_eq!(stock::completion_code(message.completion), *code);
        }
    }
}

#[test]
fn no_producer_codes_are_not_invented() {
    for code in [0x2e, 0x3b, 0x48, 0x8c, 0x9f, 0xa0] {
        assert_eq!(stock::parse(0x3b00 + code, &[]), Err(stock::ParseError::UnknownOperation));
    }
}

#[test]
fn optional_callbacks_preserve_only_present_words() {
    assert_eq!(stock::optional_callback(0), None);
    assert_eq!(stock::callback_word(stock::optional_callback(0xdefa30).unwrap()), 0xdefa30);
}

#[test]
fn notifications_keep_their_command_block_identity() {
    use crate::{ClientNotification, ReplyTarget};
    for (notification, code) in [
        (ClientNotification::Notification12, 0x12),
        (ClientNotification::Notification18, 0x18),
        (ClientNotification::Notification19, 0x19),
        (ClientNotification::Notification1b, 0x1b),
        (ClientNotification::Notification1c, 0x1c),
    ] {
        assert_eq!(stock::reply_target_code(ReplyTarget::Notification(notification)), code);
    }
    for command in stock::COMMANDS {
        assert_eq!(stock::reply_target_code(ReplyTarget::Command(command.kind)), command.code);
    }
}

#[test]
fn synchronous_lens_requests_keep_wire_operation_codes() {
    use crate::LensDcOperation;
    for (operation, code) in [
        (LensDcOperation::Request0c, 0x420c),
        (LensDcOperation::Request15, 0x4215),
        (LensDcOperation::Request16, 0x4216),
        (LensDcOperation::Request18, 0x4218),
        (LensDcOperation::Request19, 0x4219),
        (LensDcOperation::Request1a, 0x421a),
        (LensDcOperation::Request1c, 0x421c),
    ] {
        assert_eq!(stock::lens_dc_operation(operation), code);
    }
}

#[test]
fn flash_requests_preserve_wire_codes() {
    use crate::FlashOperation;
    assert_eq!(stock::flash_operation(FlashOperation::Request0d), 0x1a0d);
    assert_eq!(stock::flash_operation(FlashOperation::Request0e), 0x1a0e);
}

#[test]
fn indirect_lens_drive_and_unclassified_requests_require_the_lens_contract() {
    use crate::{lens_effect, CalcOperation, ImgDcOperation, LensDcOperation, LensEffect, PeerOperation};
    assert_eq!(lens_effect(PeerOperation::ImgDc(ImgDcOperation::Request2e)), LensEffect::DriveThroughImageBridge);
    for request in [
        PeerOperation::LensDc(LensDcOperation::Request07),
        PeerOperation::LensDc(LensDcOperation::Request0c),
        PeerOperation::ImgDc(ImgDcOperation::Request2e),
        PeerOperation::LensDc(LensDcOperation::Request00),
    ] {
        assert!(lens_effect(request).requires_lens_safety_contract());
    }
    assert!(!lens_effect(PeerOperation::Calc(CalcOperation::Request0c)).requires_lens_safety_contract());
}

#[test]
fn synchronous_record_requests_keep_wire_operation_codes() {
    assert_eq!(stock::img_dc_operation(crate::ImgDcOperation::Request56), 0x3f56);
    assert_eq!(stock::img_dc_operation(crate::ImgDcOperation::Request57), 0x3f57);
}

#[test]
fn measurement_geometry_serializes_resource_calibration_and_retained_parameters() {
    use crate::round::{GeometryCalibration, GeometryPair, MeasurementGeometry, RetainedGeometryValue};
    let geometry = MeasurementGeometry {
        resource: GeometryPair([0x1234, 0x5678]),
        calibration: GeometryCalibration([0x1122, 0x3344, 0x5566, 0x7788]),
        converted_profile: RetainedGeometryValue(0x90abcdef),
        first_parameter: RetainedGeometryValue(0x10203040),
        second_parameter: RetainedGeometryValue(0x50607080),
    };
    assert_eq!(stock::measurement_geometry(geometry), [
        16, 16, 100, 0, 100, 0, 0, 0, 0, 0, 0x34, 0x12, 0x78, 0x56, 7, 7,
        0x44, 0x33, 0x22, 0x11, 0x88, 0x77, 0x66, 0x55, 0x34, 0x12, 0x78, 0x56,
        0xef, 0xcd, 0xab, 0x90, 0x40, 0x30, 0x20, 0x10, 0x80, 0x70, 0x60, 0x50,
    ]);
    assert_eq!(stock::img_dc_operation(crate::ImgDcOperation::Request2b), 0x3f2b);
}
