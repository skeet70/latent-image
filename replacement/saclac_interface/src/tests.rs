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
    assert_eq!(lens_effect(PeerOperation::ImgDc(ImgDcOperation::Request2f)), LensEffect::DriveThroughImageBridge);
    assert_eq!(lens_effect(PeerOperation::ImgDc(ImgDcOperation::Request44)), LensEffect::DriveThroughImageBridge);
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
fn live_view_adjustment_request_keeps_its_wire_code() {
    assert_eq!(stock::img_dc_operation(crate::ImgDcOperation::Request34), 0x3f34);
}

#[test]
fn capture_layout_preserves_callback_and_converted_value_order() {
    use stock::CaptureRecordField as F;
    let mut packet = [0; 176];
    packet[..24].copy_from_slice(&[1,0,0,0,2,0,0,0,3,0,0,0,0,0,0,0,4,0,0,0,0,0,0,0]);
    packet[140..150].copy_from_slice(&[0x20,3,0xe8,3,0x90,1,0xe8,3,0x90,1]);
    packet[171..173].copy_from_slice(&[1,1]);
    assert_eq!(F::Completion.bytes(&packet), &[1,0,0,0]);
    assert_eq!(F::Auxiliary.bytes(&packet), &[2,0,0,0]);
    assert_eq!(F::Sample.bytes(&packet), &[3,0,0,0]);
    assert_eq!(F::Progress.bytes(&packet), &[4,0,0,0]);
    assert_eq!(F::ReservedHeaderA.bytes(&packet), &[0;4]);
    assert_eq!(F::ReservedHeaderB.bytes(&packet), &[0;4]);
    assert_eq!(F::ConvertedDimensions.bytes(&packet), &[0x20,3,0xe8,3,0x90,1,0xe8,3,0x90,1]);
    assert_eq!(F::OrdinaryMode.bytes(&packet), &[1]);
    assert_eq!(F::Enabled.bytes(&packet), &[1]);
    assert_eq!(stock::img_dc_operation(crate::ImgDcOperation::Request49), 0x3f49);
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

fn lens_bytes(payload: crate::lens::LensPayload, code: u16, expected: &[u8]) {
    let mut bytes = [0xaa; stock::LENS_PAYLOAD_BYTES];
    let request = stock::lens_dc_request(&payload, &mut bytes);
    assert_eq!(stock::lens_dc_operation(request.operation), code, "{payload:?}");
    assert_eq!(request.parameters.bytes(), expected, "{payload:?}");
}

#[test]
fn lens_payloads_carry_the_stock_callback_record_and_words() {
    use crate::lens::{DriveOwner, LensMode, LensPayload as P, PositionStep, ProfileSlotReference, SelectionCode, Setting};
    use crate::ParameterValue as V;
    let record = stock::record_handle_from_a2(0x0169_a000);
    lens_bytes(P::BringUp { parameters: [V(1), V(2), V(3), V(4), V(5)], record }, 0x4202, &[
        0xf0, 0x50, 0xf1, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 0, 0xa0, 0x69, 1, 4, 0, 0, 0, 5, 0, 0, 0,
    ]);
    lens_bytes(P::Stop { parameters: [V(7), V(8)], record }, 0x4203, &[
        0xc4, 0x50, 0xf1, 0, 7, 0, 0, 0, 8, 0, 0, 0, 0, 0xa0, 0x69, 1,
    ]);
    let slot = ProfileSlotReference(0x8100_1234);
    lens_bytes(P::Setting { setting: Setting::Request04, slot, mode: LensMode(13) }, 0x4204, &[
        0x50, 0x4b, 0xf1, 0, 0x34, 0x12, 0, 0x81, 13, 0, 0, 0,
    ]);
    lens_bytes(P::Setting { setting: Setting::Request05, slot, mode: LensMode(13) }, 0x4205, &[
        0x8c, 0x4b, 0xf1, 0, 0x34, 0x12, 0, 0x81, 13, 0, 0, 0,
    ]);
    lens_bytes(P::Setting { setting: Setting::Request06, slot, mode: LensMode(13) }, 0x4206, &[
        0x14, 0x4b, 0xf1, 0, 0x34, 0x12, 0, 0x81, 13, 0, 0, 0,
    ]);
    lens_bytes(P::Drive { owner: DriveOwner::Live, parameters: [V(1), V(2), V(3), V(4)], slot }, 0x4207, &[
        0xe4, 0x4b, 0xf1, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0x34, 0x12, 0, 0x81,
    ]);
    lens_bytes(P::Drive { owner: DriveOwner::S1, parameters: [V(1), V(2), V(3), V(4)], slot }, 0x4207, &[
        0x94, 0x8e, 0xf2, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0x34, 0x12, 0, 0x81,
    ]);
    lens_bytes(P::Finish { value: V(9), record }, 0x4208, &[0, 0, 0, 0, 9, 0, 0, 0, 0, 0xa0, 0x69, 1]);
    lens_bytes(P::Halt, 0x4209, &[]);
    lens_bytes(P::Release, 0x4212, &[]);
    lens_bytes(P::Step(PositionStep(-2)), 0x420b, &[
        0xc8, 0x4b, 0xf1, 0, 0xfe, 0xff, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0,
    ]);
    lens_bytes(P::Select { value: V(6), code: SelectionCode::Configured, record }, 0x4211, &[
        0xe4, 0x4a, 0xf1, 0, 6, 0, 0, 0, 12, 0, 0, 0, 0, 0xa0, 0x69, 1,
    ]);
    lens_bytes(P::Select { value: V(6), code: SelectionCode::Unconfigured, record }, 0x4211, &[
        0xe4, 0x4a, 0xf1, 0, 6, 0, 0, 0, 11, 0, 0, 0, 0, 0xa0, 0x69, 1,
    ]);
}

#[test]
fn lens_payload_completions_are_the_posts_of_the_callback_it_names() {
    use crate::lens::{DriveOwner, LensMode, LensPayload as P, PositionStep, ProfileSlotReference, SelectionCode, Setting};
    use crate::ParameterValue as V;
    let record = stock::record_handle_from_a2(1);
    let slot = ProfileSlotReference(0);
    let payloads = [
        P::BringUp { parameters: [V(0); 5], record },
        P::Stop { parameters: [V(0); 2], record },
        P::Setting { setting: Setting::Request04, slot, mode: LensMode(0) },
        P::Setting { setting: Setting::Request05, slot, mode: LensMode(0) },
        P::Setting { setting: Setting::Request06, slot, mode: LensMode(0) },
        P::Drive { owner: DriveOwner::Live, parameters: [V(0); 4], slot },
        P::Drive { owner: DriveOwner::S1, parameters: [V(0); 4], slot },
        P::Finish { value: V(0), record },
        P::Halt,
        P::Step(PositionStep(0)),
        P::Select { value: V(0), code: SelectionCode::Unconfigured, record },
        P::Release,
    ];
    for payload in payloads {
        let word = stock::lens_callback_word(&payload);
        let posts = payload.completions();
        match stock::CALLBACKS.iter().find(|entry| entry.word == word) {
            Some(entry) => {
                assert_eq!(posts.len(), entry.posts.len(), "{payload:?}");
                assert!(posts.iter().all(|completion| entry.posts.contains(&stock::completion_code(*completion))), "{payload:?}");
            }
            None => assert!(word == 0 && posts.is_empty(), "{payload:?}"),
        }
    }
}

#[test]
fn peer_queue_blocks_hold_the_longest_stock_payloads() {
    use crate::calc::{CalcPayload, SharedAddress};
    use stock::{peers, shared, StockCallbackWord};
    let (_, calculation) = stock::calc_payload(&CalcPayload::Calculation {
        block: SharedAddress(0),
        source: SharedAddress(0),
        callback: StockCallbackWord(0),
    });
    for (bytes, route) in [
        (stock::LENS_PAYLOAD_BYTES, peers::LENS_DC),
        (calculation.bytes().len(), peers::CALC),
        (shared::LENS_COM_BYTES, peers::LENS_COM),
        (shared::IMAGE_PROCESSOR_TRIPLET_BYTES, peers::IMAGE_PROCESSOR),
    ] {
        assert!(bytes <= route.payload_capacity(), "{route:?}");
    }
}
