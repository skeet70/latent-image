use xm1_saclac_interface::CalcSubOperation;

use super::fakes::{ClassificationEvent, ClassificationPeer, CLASSIFICATION_COMPLETE, CLASSIFICATION_FRAME};
use super::SceneRecognition;
use crate::exposure::fakes::World;

fn words(bytes: &[u8; 48]) -> [u32; 12] {
    core::array::from_fn(|index| u32::from_le_bytes(bytes[4 * index..4 * index + 4].try_into().unwrap()))
}

fn posted(peer: &ClassificationPeer, index: usize) -> [u32; 12] {
    let ClassificationEvent::Post(CalcSubOperation::Request00, bytes) = &peer.events[2 * index + 1] else { panic!("expected classification") };
    words(bytes)
}

#[test]
fn classification_enables_the_peer_then_posts_all_twelve_words() {
    let mut state = SceneRecognition::new();
    state.mark_adjustments_changed();
    let mut peer = ClassificationPeer::default();
    state.request_classification(&mut peer, CLASSIFICATION_FRAME, [16, 9], 3, CLASSIFICATION_COMPLETE).unwrap();
    let expected: [u32; 12] = [
        0x0170_1234, 0x0171_5678, 2, 2, 0x4172_9abc, 0x0173_1358, 0x0174_2468, 1, 7, 0x4175_abcd, 1, 0x0102_df28,
    ];
    let bytes = core::array::from_fn(|index| expected[index / 4].to_le_bytes()[index % 4]);
    assert_eq!(peer.events, [ClassificationEvent::Enable, ClassificationEvent::Post(CalcSubOperation::Request00, bytes)]);
}

#[test]
fn classification_maps_recognised_and_unrecognised_caller_modes() {
    for (aspect, orientation, aspect_code, orientation_code) in [
        ([3, 2], 2, 1, 1),
        ([16, 9], 3, 2, 2),
        ([1, 1], 4, 3, 3),
        ([3, 1], 1, 0, 0),
        ([16, 2], 0, 0, 0),
        ([1, 9], 5, 0, 0),
        ([2, 3], u32::MAX, 0, 0),
        ([u16::MAX, u16::MAX], 0x1_0002, 0, 0),
    ] {
        let mut state = SceneRecognition::new();
        let mut peer = ClassificationPeer::default();
        state.request_classification(&mut peer, CLASSIFICATION_FRAME, aspect, orientation, CLASSIFICATION_COMPLETE).unwrap();
        let posted = posted(&peer, 0);
        assert_eq!((posted[2], posted[3]), (aspect_code, orientation_code), "{aspect:?}, {orientation}");
    }
}

#[test]
fn reset_and_changed_marks_reach_one_post_even_when_the_peer_refuses() {
    for refuse in [false, true] {
        let mut state = SceneRecognition::new();
        let mut peer = ClassificationPeer { refuse, ..ClassificationPeer::default() };
        let mut world = World::default();
        let expected_result = if refuse { Err(()) } else { Ok(()) };
        for (index, mark) in [0, 1, 0, 1, 0].into_iter().enumerate() {
            if index == 1 { state.reset(&mut world); }
            if index == 3 { state.mark_adjustments_changed(); }
            assert_eq!(state.request_classification(&mut peer, CLASSIFICATION_FRAME, [3, 2], 1, CLASSIFICATION_COMPLETE), expected_result);
            assert_eq!(posted(&peer, index)[10], mark);
        }
    }
}

#[test]
fn classification_preserves_the_receivers_averaging_selector_word() {
    let mut state = SceneRecognition::new();
    let mut peer = ClassificationPeer::default();
    let mut frame = CLASSIFICATION_FRAME;
    frame.average_levels = u32::MAX;
    state.request_classification(&mut peer, frame, [3, 2], 1, CLASSIFICATION_COMPLETE).unwrap();
    assert_eq!(posted(&peer, 0)[7], u32::MAX);
}
