use super::*;
use crate::shutdown::RequestId;
use crate::storage_shutdown_lens_service::LensCallback;
use std::vec::Vec;

struct Software {
    pending: Vec<LensPendingCommand>,
    transmitted: Vec<LensCommandWord>,
    diagnostics: Vec<LensControlFailure>,
    replies: Vec<(LensCallback, LensResponseStatus, LensReplyData)>,
    observers: Vec<LensResponseStatus>,
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
    fn notify_observer(&mut self, status: LensResponseStatus) { self.observers.push(status) }
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
fn reply_requires_the_last_command_and_a_callback() {
    let mut software = Software::new();
    dispatch_reply(&mut software, pending(0x7202, false, 0, 0x44), LensResponseStatus(0), 0);
    dispatch_reply(&mut software, pending(0x7202, true, 0, 0), LensResponseStatus(0), 0);
    assert!(software.replies.is_empty());
    dispatch_reply(&mut software, pending(0x7202, true, 0, 0x44), LensResponseStatus(0), 0);
    assert_eq!(software.replies, std::vec![(LensCallback(0x44), LensResponseStatus(0), LensReplyData::StatusOnly)]);
}

#[test]
fn channel_operation_splits_the_response_word() {
    let mut software = Software::new();
    let response = (0x123u32 << 16) | (5 << 27) | (1 << 31);
    dispatch_reply(&mut software, pending(0x722e, true, 0, 0x44), LensResponseStatus(2), response);
    assert_eq!(
        software.replies,
        std::vec![(LensCallback(0x44), LensResponseStatus(2), LensReplyData::Channel { pages: 0x123, flags: 5, present: 1 })]
    );
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
    enqueue_control(&mut software, &context);
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
    enqueue_control(&mut software, &context);
    assert!(software.pending.is_empty());
    assert_eq!(software.diagnostics, std::vec![LensControlFailure::QueueFull, LensControlFailure::QueueFull]);
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
    enqueue_control(&mut software, &context);
    assert_eq!(software.pending, std::vec![pending(0x7202, true, 0, 0x44), skipped(0x7202, true, 0x200, 0)]);
    assert!(software.transmitted.is_empty());
}

#[test]
fn skipped_command_without_a_pending_command_is_answered_immediately() {
    let mut context = context();
    put_word(&mut context, 0x170, 1);
    put_word(&mut context, 0x0c, 0x44);
    put_word(&mut context, 0xe0, 0x99);
    context[0x38] = 1;
    put_word(&mut context, 0x10, 0x200);
    let mut software = Software::new();
    enqueue_control(&mut software, &context);
    assert!(software.pending.is_empty());
    assert!(software.transmitted.is_empty());
    assert_eq!(software.replies, std::vec![(LensCallback(0x44), LensResponseStatus(0), LensReplyData::StatusOnly)]);
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
    assert_eq!(software.observers, std::vec![LensResponseStatus(3)]);
}

#[test]
fn read_status_override_reports_four_when_the_length_halfword_is_set() {
    let mut context = context();
    put_word(&mut context, 0x0c, 0x0001_0000);
    let mut software = Software::with_pending(std::vec![pending(0x7217, true, 0, 0x44)]);
    assert!(command_response(&mut software, &mut context));
    assert_eq!(software.observers, std::vec![LensResponseStatus(4)]);
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
