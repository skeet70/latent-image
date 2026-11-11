use super::*;
use crate::shutdown::RequestId;
use crate::storage_shutdown_lens_service::LensCallback;
use std::vec::Vec;

struct Software {
    pending: Vec<LensPendingCommand>,
    transmitted: Vec<LensCommandWord>,
    diagnostics: Vec<LensControlFailure>,
    replies: Vec<(LensCallback, LensResponseStatus, LensReplyData)>,
    observers: Vec<(LensResponseStatus, [u8; 9])>,
    full: bool,
}
impl Software {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
            transmitted: Vec::new(),
            diagnostics: Vec::new(),
            replies: Vec::new(),
            observers: Vec::new(),
            full: false,
        }
    }
    fn with_pending(pending: Vec<LensPendingCommand>) -> Self {
        Self { pending, ..Self::new() }
    }
}
impl LensControlSoftware for Software {
    fn push(&mut self, command: LensPendingCommand) -> bool {
        if self.full { return false; }
        self.pending.push(command);
        true
    }
    fn peek(&mut self) -> Option<LensPendingCommand> { self.pending.first().copied() }
    fn pop(&mut self) -> Option<LensPendingCommand> {
        if self.pending.is_empty() { None } else { Some(self.pending.remove(0)) }
    }
    fn transmit(&mut self, command: LensCommandWord) { self.transmitted.push(command) }
    fn diagnostic(&mut self, failure: LensControlFailure) { self.diagnostics.push(failure) }
    fn reply(&mut self, callback: LensCallback, status: LensResponseStatus, data: LensReplyData) {
        self.replies.push((callback, status, data));
    }
    fn notify_observer(&mut self, status: LensResponseStatus, bits: &[u8; 9]) {
        self.observers.push((status, *bits));
    }
}

fn pending(operation: u16, last: bool, command: u32, callback: u32) -> LensPendingCommand {
    LensPendingCommand::new(RequestId(operation), last, command, LensCallback(callback), false)
}

fn skipped(operation: u16, last: bool, command: u32, callback: u32) -> LensPendingCommand {
    LensPendingCommand::new(RequestId(operation), last, command, LensCallback(callback), true)
}

fn context() -> [u8; 0x174] { [0u8; 0x174] }

fn put_word(context: &mut [u8; 0x174], offset: usize, value: u32) {
    context[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// 0x6b86d8, transcribed from the listing.
const TABLE: [(u16, ReplyBody); 91] = [
    (0x7202, ReplyBody::Status), (0x7203, ReplyBody::Status), (0x7204, ReplyBody::Status),
    (0x7205, ReplyBody::Status), (0x7206, ReplyBody::Status), (0x7207, ReplyBody::Status),
    (0x7208, ReplyBody::Status), (0x7209, ReplyBody::Bits16To18), (0x720a, ReplyBody::Status),
    (0x720b, ReplyBody::Status), (0x720c, ReplyBody::Wide14), (0x720d, ReplyBody::ResponseHalfword),
    (0x720e, ReplyBody::Bit31And14), (0x720f, ReplyBody::Bit31AndSigned15),
    (0x7210, ReplyBody::Bit31SevenAndByte), (0x7211, ReplyBody::ByteAndBit31),
    (0x7212, ReplyBody::Bits16To18), (0x7213, ReplyBody::ResponseHalfword),
    (0x7214, ReplyBody::Bits16To27), (0x7215, ReplyBody::Bits16To31),
    (0x7216, ReplyBody::ResponseHalfword), (0x7217, ReplyBody::ContextBits9),
    (0x7218, ReplyBody::Status), (0x7219, ReplyBody::Status), (0x721a, ReplyBody::Status),
    (0x721b, ReplyBody::None), (0x721c, ReplyBody::None), (0x721d, ReplyBody::Status),
    (0x721e, ReplyBody::Status), (0x721f, ReplyBody::Status), (0x7220, ReplyBody::Status),
    (0x7221, ReplyBody::Status), (0x7222, ReplyBody::Status), (0x7223, ReplyBody::Status),
    (0x7224, ReplyBody::Status), (0x7225, ReplyBody::Status), (0x7226, ReplyBody::Status),
    (0x7227, ReplyBody::Status), (0x7228, ReplyBody::Status), (0x7229, ReplyBody::Status),
    (0x722a, ReplyBody::Status), (0x722b, ReplyBody::Status), (0x722c, ReplyBody::Status),
    (0x722d, ReplyBody::Status), (0x722e, ReplyBody::Channel), (0x722f, ReplyBody::Status),
    (0x7230, ReplyBody::Status), (0x7231, ReplyBody::Status), (0x7232, ReplyBody::Status),
    (0x7233, ReplyBody::Status), (0x7234, ReplyBody::Status), (0x7235, ReplyBody::Status),
    (0x7236, ReplyBody::Status), (0x7237, ReplyBody::Status), (0x7238, ReplyBody::Bits16To31),
    (0x7239, ReplyBody::ResponseHalfword), (0x723a, ReplyBody::Bits16To30AndBit31),
    (0x723b, ReplyBody::ResponseHalfword), (0x723c, ReplyBody::ResponseHalfword),
    (0x723d, ReplyBody::ResponseHalfword), (0x723e, ReplyBody::ResponseHalfword),
    (0x723f, ReplyBody::Status), (0x7240, ReplyBody::Status), (0x7241, ReplyBody::Status),
    (0x7242, ReplyBody::Status), (0x7243, ReplyBody::Status), (0x7244, ReplyBody::Status),
    (0x7245, ReplyBody::Status), (0x7246, ReplyBody::Status), (0x7247, ReplyBody::Status),
    (0x7248, ReplyBody::Status), (0x7249, ReplyBody::Status), (0x724a, ReplyBody::Status),
    (0x724b, ReplyBody::Status), (0x724c, ReplyBody::Status), (0x724d, ReplyBody::Status),
    (0x724e, ReplyBody::Status), (0x724f, ReplyBody::Status), (0x7250, ReplyBody::Status),
    (0x7251, ReplyBody::None), (0x7252, ReplyBody::None), (0x7253, ReplyBody::None),
    (0x7254, ReplyBody::None), (0x7255, ReplyBody::None), (0x7256, ReplyBody::ResponseHalfword),
    (0x7257, ReplyBody::Status), (0x7258, ReplyBody::Status), (0x7259, ReplyBody::Status),
    (0x725a, ReplyBody::Status), (0x725b, ReplyBody::Status), (0x725c, ReplyBody::Status),
];

fn operations(body: ReplyBody) -> impl Iterator<Item = u16> {
    TABLE.into_iter().filter(move |(_, entry)| *entry == body).map(|(operation, _)| operation)
}

fn reply_for(operation: u16, response: u32) -> Option<LensReplyData> {
    let mut software = Software::new();
    let mut context = context();
    dispatch_reply(&mut software, &mut context, pending(operation, true, 0, 0x44), LensResponseStatus(2), response);
    software.replies.first().map(|(_, _, data)| *data)
}

const RESPONSE: u32 = 0x8F3C_A5C3;

#[test]
fn status_operations_pass_only_the_status() {
    for operation in operations(ReplyBody::Status) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::StatusOnly), "op {operation:#06x}");
    }
}

#[test]
fn bits_16_to_18_operations_pass_three_bits() {
    for operation in operations(ReplyBody::Bits16To18) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::Bits16To18([0, 0, 1])), "op {operation:#06x}");
    }
}

#[test]
fn wide_operation_passes_fourteen_decoded_fields() {
    for operation in operations(ReplyBody::Wide14) {
        assert_eq!(
            reply_for(operation, RESPONSE),
            Some(LensReplyData::Wide14([1, 0, 0, 0, 1, 3, 0, 0, 1, 1, 1, 1, 0, 0])),
            "op {operation:#06x}"
        );
    }
}

#[test]
fn halfword_operations_pass_the_word_at_response_plus_two() {
    for operation in operations(ReplyBody::ResponseHalfword) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::ResponseHalfword([0x3c, 0x8f])), "op {operation:#06x}");
    }
}

#[test]
fn bit_31_and_fourteen_operations_pass_both_fields() {
    for operation in operations(ReplyBody::Bit31And14) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::Bit31And14([1, 0, 0x3c, 0x0f])), "op {operation:#06x}");
    }
}

#[test]
fn signed_fifteen_operation_sign_extends_bit_fourteen() {
    for operation in operations(ReplyBody::Bit31AndSigned15) {
        assert_eq!(
            reply_for(operation, 0xCF3C_A5C3),
            Some(LensReplyData::Bit31AndSigned15([1, 0, 0x3c, 0xcf])),
            "op {operation:#06x}"
        );
    }
}

#[test]
fn seven_bit_operation_passes_three_bytes() {
    for operation in operations(ReplyBody::Bit31SevenAndByte) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::Bit31SevenAndByte([1, 0x0f, 0x3c])), "op {operation:#06x}");
    }
}

#[test]
fn byte_and_bit_31_operation_passes_two_bytes() {
    for operation in operations(ReplyBody::ByteAndBit31) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::ByteAndBit31([0x3c, 1])), "op {operation:#06x}");
    }
}

#[test]
fn twelve_bit_operation_passes_two_bytes() {
    for operation in operations(ReplyBody::Bits16To27) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::Bits16To27([0x3c, 0x0f])), "op {operation:#06x}");
    }
}

#[test]
fn sixteen_bit_operations_pass_sixteen_bytes() {
    for operation in operations(ReplyBody::Bits16To31) {
        assert_eq!(
            reply_for(operation, RESPONSE),
            Some(LensReplyData::Bits16To31([0, 0, 1, 1, 1, 1, 0, 0, 1, 1, 1, 1, 0, 0, 0, 1])),
            "op {operation:#06x}"
        );
    }
}

#[test]
fn read_operation_stores_nine_bits_at_context_120_and_passes_them() {
    for operation in operations(ReplyBody::ContextBits9) {
        let mut software = Software::new();
        let mut context = context();
        dispatch_reply(&mut software, &mut context, pending(operation, true, 0, 0x44), LensResponseStatus(2), RESPONSE);
        let bits = [0, 0, 1, 1, 1, 1, 0, 1, 0];
        assert_eq!(&context[0x120..0x129], &bits, "op {operation:#06x}");
        assert_eq!(software.replies, std::vec![(LensCallback(0x44), LensResponseStatus(2), LensReplyData::ContextBits9(bits))]);
    }
}

#[test]
fn channel_operation_splits_the_response_word() {
    for operation in operations(ReplyBody::Channel) {
        assert_eq!(
            reply_for(operation, RESPONSE),
            Some(LensReplyData::Channel { pages: 0x73c, flags: 1, present: 1 }),
            "op {operation:#06x}"
        );
    }
}

#[test]
fn fifteen_bit_operation_passes_three_bytes() {
    for operation in operations(ReplyBody::Bits16To30AndBit31) {
        assert_eq!(reply_for(operation, RESPONSE), Some(LensReplyData::Bits16To30AndBit31([0x3c, 0x0f, 1])), "op {operation:#06x}");
    }
}

#[test]
fn no_call_operations_leave_the_callback_alone() {
    let mut called = 0;
    for operation in operations(ReplyBody::None) {
        called += 1;
        assert_eq!(reply_for(operation, RESPONSE), None, "op {operation:#06x}");
    }
    assert_eq!(called, 7);
}

#[test]
fn an_operation_outside_the_table_is_not_answered() {
    assert_eq!(reply_for(0x7201, RESPONSE), None);
    assert_eq!(reply_for(0x725d, RESPONSE), None);
}

#[test]
fn reply_requires_the_last_command_and_a_callback() {
    let mut software = Software::new();
    let mut context = context();
    dispatch_reply(&mut software, &mut context, pending(0x7202, false, 0, 0x44), LensResponseStatus(0), 0);
    dispatch_reply(&mut software, &mut context, pending(0x7202, true, 0, 0), LensResponseStatus(0), 0);
    assert!(software.replies.is_empty());
    dispatch_reply(&mut software, &mut context, pending(0x7202, true, 0, 0x44), LensResponseStatus(0), 0);
    assert_eq!(software.replies, std::vec![(LensCallback(0x44), LensResponseStatus(0), LensReplyData::StatusOnly)]);
}

#[test]
fn cleared_skip_word_never_skips_a_plain_operation() {
    let mut context = context();
    assert!(!skip_command(&context, RequestId(0x7202), LensCommandWord(0x200)));
    put_word(&mut context, 0x170, 0);
    assert!(!skip_command(&context, RequestId(0x7202), LensCommandWord(0x200)));
}

#[test]
fn skip_requires_a_live_skip_word_and_a_clear_entry() {
    let mut context = context();
    put_word(&mut context, 0x170, 1);
    assert!(skip_command(&context, RequestId(0x7202), LensCommandWord(0x200)));
    context[0x12d] = 1;
    assert!(!skip_command(&context, RequestId(0x7202), LensCommandWord(0x200)));
}

#[test]
fn prepare_primary_is_skipped_only_when_the_saved_halfword_matches() {
    let mut context = context();
    context[0x12a] = 0x34;
    context[0x12b] = 0x12;
    assert!(skip_command(&context, RequestId(0x722b), LensCommandWord(0x1234_0000)));
    assert!(!skip_command(&context, RequestId(0x722b), LensCommandWord(0x4321_0000)));
}

#[test]
fn extended_is_skipped_only_for_a_plain_mode_nibble() {
    let mut context = context();
    assert!(skip_command(&context, RequestId(0x722d), LensCommandWord(0x0001_0000)));
    assert!(!skip_command(&context, RequestId(0x722d), LensCommandWord(0x0004_0000)));
    assert!(!skip_command(&context, RequestId(0x722d), LensCommandWord(0)));
    put_word(&mut context, 0x16c, 1);
    assert!(!skip_command(&context, RequestId(0x722d), LensCommandWord(0x0001_0000)));
}

#[test]
fn enqueue_control_pushes_transmits_and_appends_the_follow_on_command() {
    let mut context = context();
    put_word(&mut context, 0x0c, 0x44);
    context[0..2].copy_from_slice(&0x7202u16.to_le_bytes());
    context[0x38] = 2;
    put_word(&mut context, 0x10, 0x100);
    put_word(&mut context, 0x14, 0x1000);
    let mut software = Software::new();
    enqueue_control(&mut software, &mut context);
    assert_eq!(
        software.pending,
        std::vec![
            pending(0x7202, false, 0x100, 0x44),
            pending(0x7202, true, 0x1000, 0x44),
            pending(0x7202, true, 0x3fc0, 0),
        ]
    );
    assert_eq!(software.transmitted, std::vec![LensCommandWord(0x100), LensCommandWord(0x1000), LensCommandWord(0x3fc0)]);
    assert!(software.diagnostics.is_empty());
}

#[test]
fn enqueue_control_diagnoses_a_full_queue_and_still_transmits() {
    let mut context = context();
    put_word(&mut context, 0x0c, 0x44);
    context[0x38] = 1;
    put_word(&mut context, 0x10, 0x1000);
    let mut software = Software::new();
    software.full = true;
    enqueue_control(&mut software, &mut context);
    assert!(software.pending.is_empty());
    assert_eq!(software.diagnostics, std::vec![LensControlFailure::QueueFull(1), LensControlFailure::QueueFull(2)]);
    assert_eq!(software.transmitted, std::vec![LensCommandWord(0x1000), LensCommandWord(0x3fc0)]);
}

#[test]
fn skipped_command_is_queued_while_another_command_is_pending() {
    let mut context = context();
    put_word(&mut context, 0x170, 1);
    context[0..2].copy_from_slice(&0x7202u16.to_le_bytes());
    context[0x38] = 1;
    put_word(&mut context, 0x10, 0x200);
    let mut software = Software::with_pending(std::vec![pending(0x7202, true, 0, 0x44)]);
    enqueue_control(&mut software, &mut context);
    assert_eq!(software.pending, std::vec![pending(0x7202, true, 0, 0x44), skipped(0x7202, true, 0x200, 0)]);
    assert!(software.transmitted.is_empty());
}

#[test]
fn skipped_command_without_a_pending_command_is_answered_immediately() {
    let mut context = context();
    put_word(&mut context, 0x170, 1);
    put_word(&mut context, 0x0c, 0x44);
    put_word(&mut context, 0xe0, 0x99);
    context[0..2].copy_from_slice(&0x7202u16.to_le_bytes());
    context[0x38] = 1;
    put_word(&mut context, 0x10, 0x200);
    let mut software = Software::new();
    enqueue_control(&mut software, &mut context);
    assert!(software.pending.is_empty());
    assert!(software.transmitted.is_empty());
    assert_eq!(software.replies, std::vec![(LensCallback(0x44), LensResponseStatus(0), LensReplyData::StatusOnly)]);
}

#[test]
fn a_full_skipped_push_reports_argument_zero() {
    let mut context = context();
    put_word(&mut context, 0x170, 1);
    context[0x38] = 1;
    put_word(&mut context, 0x10, 0x200);
    let mut software = Software::with_pending(std::vec![pending(0x7202, true, 0, 0x44)]);
    software.full = true;
    enqueue_control(&mut software, &mut context);
    assert_eq!(software.diagnostics, std::vec![LensControlFailure::QueueFull(0)]);
}

#[test]
fn response_word_is_stored_before_the_pending_command_is_dispatched() {
    let mut context = context();
    put_word(&mut context, 0x0c, 0x55);
    let mut software = Software::with_pending(std::vec![pending(0x7202, false, 0, 0x44)]);
    assert!(!command_response(&mut software, &mut context));
    assert_eq!(context[0xdc], 1);
    assert_eq!(&context[0xb4..0xb8], &0x55u32.to_le_bytes());
    assert!(software.replies.is_empty());
    assert!(software.diagnostics.is_empty());
}

#[test]
fn missing_response_is_diagnosed_without_a_reply() {
    let mut context = context();
    let mut software = Software::new();
    assert!(!command_response(&mut software, &mut context));
    assert_eq!(software.diagnostics, std::vec![LensControlFailure::MissingResponse]);
    assert!(software.replies.is_empty());
    assert!(software.observers.is_empty());
}

#[test]
fn missing_response_with_a_nonzero_status_still_notifies_the_observer() {
    let mut context = context();
    put_word(&mut context, 0x10, 7);
    let mut software = Software::new();
    assert!(command_response(&mut software, &mut context));
    assert_eq!(software.observers, std::vec![(LensResponseStatus(7), [0; 9])]);
}

#[test]
fn response_overflow_is_diagnosed_before_the_pending_command_is_dropped() {
    let mut context = context();
    context[0xdc] = 10;
    let mut software = Software::new();
    assert!(!command_response(&mut software, &mut context));
    assert_eq!(
        software.diagnostics,
        std::vec![LensControlFailure::ResponseOverflow, LensControlFailure::MissingResponse]
    );
    assert_eq!(context[0xdc], 10);
}

#[test]
fn last_command_resets_the_response_count_and_replies() {
    let mut context = context();
    context[0xdc] = 3;
    put_word(&mut context, 0x0c, 0x77);
    let mut software = Software::with_pending(std::vec![pending(0x7202, true, 0, 0x44)]);
    assert!(!command_response(&mut software, &mut context));
    assert_eq!(context[0xdc], 0);
    assert_eq!(
        software.replies,
        std::vec![(LensCallback(0x44), LensResponseStatus(0), LensReplyData::StatusOnly)]
    );
}

#[test]
fn nonzero_status_notifies_the_observer() {
    let mut context = context();
    put_word(&mut context, 0x10, 3);
    let mut software = Software::with_pending(std::vec![pending(0x7202, false, 0, 0x44)]);
    assert!(command_response(&mut software, &mut context));
    assert_eq!(software.observers, std::vec![(LensResponseStatus(3), [0; 9])]);
}

#[test]
fn read_status_override_reports_four_when_the_length_halfword_is_set() {
    let mut context = context();
    put_word(&mut context, 0x0c, 0x0001_0000);
    let mut software = Software::with_pending(std::vec![pending(0x7217, true, 0, 0x44)]);
    assert!(command_response(&mut software, &mut context));
    assert_eq!(software.observers.len(), 1);
    assert_eq!(software.observers[0].0, LensResponseStatus(4));
}

#[test]
fn prepare_primary_response_records_the_saved_halfword() {
    let mut context = context();
    let mut software = Software::with_pending(std::vec![pending(0x722b, true, 0x1234_0000, 0x44)]);
    assert!(!command_response(&mut software, &mut context));
    assert_eq!(&context[0x12a..0x12c], &0x1234u16.to_le_bytes());
}

#[test]
fn skipped_entries_are_drained_with_the_echo_word() {
    let mut context = context();
    put_word(&mut context, 0x0c, 0x11);
    put_word(&mut context, 0xe0, 0x22);
    let mut software = Software::with_pending(std::vec![
        pending(0x7202, false, 0, 0x44),
        skipped(0x7202, true, 0, 0x44),
    ]);
    assert!(!command_response(&mut software, &mut context));
    assert!(software.pending.is_empty());
    assert_eq!(
        software.replies,
        std::vec![(LensCallback(0x44), LensResponseStatus(0), LensReplyData::StatusOnly)]
    );
    assert_eq!(context[0xdc], 0);
}
