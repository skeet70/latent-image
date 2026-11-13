use crate::round_image::{CancelArgument, ImageArea, ImageArguments, ImageChannel, ImageLaneRequest, SnapshotArgument};
use crate::stock::round_image::{image_failed, image_lane_request};
use crate::stock::{img_dc_operation, StockCallbackWord};

const FINISHED: StockCallbackWord = StockCallbackWord(0x00f3_9b64);

fn assert_encoded(request: ImageLaneRequest, operation: u16, bytes: &[u8]) {
    let (encoded, message) = image_lane_request(&request, FINISHED);
    assert_eq!((img_dc_operation(encoded), message.bytes()), (operation, bytes));
}

#[test]
fn an_image_request_carries_the_callback_the_client_words_the_area_and_the_channel() {
    let request = ImageLaneRequest::Image {
        arguments: ImageArguments([0x1111_2222, 0x3333_4444]),
        area: ImageArea([-2, 0x0506]),
        channel: ImageChannel(7),
    };
    assert_encoded(request, 0x3f30, &[
        0x64, 0x9b, 0xf3, 0x00, 0x22, 0x22, 0x11, 0x11, 0x44, 0x44, 0x33, 0x33,
        0xfe, 0xff, 0x06, 0x05, 7, 0, 0, 0,
    ]);
}

#[test]
fn a_snapshot_request_leaves_bytes_ten_and_eleven_zero() {
    let request = ImageLaneRequest::Snapshot { argument: SnapshotArgument(-3), area: ImageArea([0x0102, 0x0304]), channel: ImageChannel(0x0a0b_0c0d) };
    assert_encoded(request, 0x3f33, &[
        0x64, 0x9b, 0xf3, 0x00, 0xfd, 0xff, 0x02, 0x01, 0x04, 0x03, 0, 0, 0x0d, 0x0c, 0x0b, 0x0a,
    ]);
}

#[test]
fn cancel_carries_its_argument_and_stop_a_zero_word() {
    assert_encoded(ImageLaneRequest::Cancel(CancelArgument(0x0102_0304)), 0x3f32, &[4, 3, 2, 1]);
    assert_encoded(ImageLaneRequest::Stop, 0x3f31, &[0; 4]);
}

#[test]
fn an_image_fails_on_any_nonzero_status_word() {
    assert!(!image_failed(&[0, 0, 0, 0, 0xff, 0xff, 0xff, 0xff]));
    assert!(image_failed(&[0, 0, 0, 0x80]));
    assert!(image_failed(&[1, 0, 0, 0]));
    assert!(!image_failed(&[]));
}
