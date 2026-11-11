use super::*;

#[derive(Default)]
struct Queue {
    messages: std::vec::Vec<(RequestId, TaskId, QueueId, std::vec::Vec<u8>)>,
}
impl ShutdownRtos for Queue {
    fn current_task(&mut self) -> TaskId { TaskId(7) }
    fn post(&mut self, message: ServiceMessage<'_>) -> Result<(), KernelStatus> {
        self.messages.push((message.operation, message.sender, message.destination, message.payload.to_vec()));
        Ok(())
    }
    fn set_event_flag(&mut self, _: u32, _: u32) -> Result<(), KernelStatus> { unreachable!() }
}

#[test]
fn task7_request_copies_callback_word_and_explicit_sender() {
    let mut queue = Queue::default();
    assert_eq!(task7_shutdown(&mut queue, TaskId(2), UsbCompletionWord(0x12345678)), Ok(()));
    assert_eq!(queue.messages, [(RequestId(0x1701), TaskId(2), QueueId(7), std::vec![0x78, 0x56, 0x34, 0x12])]);
}

#[test]
fn usb_release_forwards_every_residue_byte_without_editing_it() {
    let residue = core::array::from_fn(|index| index as u8 ^ 0x97);
    let mut queue = Queue::default();
    assert_eq!(request_usb_release(&mut queue, &residue), Ok(()));
    assert_eq!(queue.messages, [(RequestId(0x3401), TaskId(7), QueueId(36), residue.to_vec())]);
}

#[test]
fn actual_release_acknowledgement_posts_empty_b401_to_task7() {
    let mut queue = Queue::default();
    assert_eq!(usb_release_acknowledged(&mut queue), Ok(()));
    assert_eq!(queue.messages, [(RequestId(0xb401), TaskId(7), QueueId(7), std::vec![])]);
}
