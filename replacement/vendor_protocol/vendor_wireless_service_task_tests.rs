use super::vendor_wireless_service_task::{CompletionRegistration, CompletionStatus, CompletionTarget, InterfaceOperation, MessageClass, PayloadError, TaskError, TaskPayload, TaskRuntime, TaskState};

#[test]
fn wire_message_classes_distinguish_control_and_completion_messages() {
    assert_eq!(MessageClass::parse(0x660b), MessageClass::Operation);
    assert_eq!(MessageClass::parse(0xe60b), MessageClass::OperationResult);
    assert_eq!(MessageClass::parse(0x662a), MessageClass::ControlResult);
    assert_eq!(MessageClass::parse(0x6609), MessageClass::Completion);
    assert_eq!(MessageClass::parse(0x660a), MessageClass::Event);
    assert_eq!(MessageClass::parse(0x6610), MessageClass::Other);
}

#[test]
fn rejected_operations_deliver_registered_protocol_notifications_in_order() {
    let mut task = TaskRuntime::new();
    task.register(CompletionTarget::Operation, CompletionRegistration(11));
    task.register(CompletionTarget::OperationReset, CompletionRegistration(12));
    task.register(CompletionTarget::OperationDone, CompletionRegistration(13));
    task.reject_operation(InterfaceOperation::ConfigureMedia).unwrap();
    task.reject_operation(InterfaceOperation::Reset).unwrap();
    task.reject_operation(InterfaceOperation::Start).unwrap();
    for (registration, target) in [(11, CompletionTarget::Operation), (12, CompletionTarget::OperationReset), (13, CompletionTarget::OperationDone)] {
        let notification = task.notification().unwrap();
        assert_eq!(notification.registration, CompletionRegistration(registration));
        assert_eq!(notification.target, target);
        assert_eq!(notification.status, CompletionStatus::NotInitialized);
        assert_eq!(notification.published_phase, TaskState::S0);
    }
    assert_eq!(task.notification(), None);
    assert_eq!(task.state(), TaskState::S0);
}

#[test]
fn disabled_interface_rejections_do_not_enable_the_task() {
    let mut task = TaskRuntime::new();
    for target in [CompletionTarget::Deinitialize, CompletionTarget::Start, CompletionTarget::Stop, CompletionTarget::Reset] {
        task.register(target, CompletionRegistration(9));
    }
    task.reject_deinitialize().unwrap();
    task.reject_start().unwrap();
    task.reject_stop().unwrap();
    task.reject_reset().unwrap();
    for target in [CompletionTarget::Deinitialize, CompletionTarget::Start, CompletionTarget::Stop, CompletionTarget::Reset] {
        let notification = task.notification().unwrap();
        assert_eq!(notification.target, target);
        assert_eq!(notification.status.raw(), -10);
    }
    assert_eq!(task.state(), TaskState::S0);
    assert_eq!(task.phase(), TaskState::S0);
}

#[test]
fn late_completions_are_ignored_when_no_operation_is_registered() {
    let mut task = TaskRuntime::new();
    task.register(CompletionTarget::Start, CompletionRegistration(4));
    task.operation_result(CompletionStatus::Success).unwrap();
    task.complete_configuration().unwrap();
    task.complete_interface_initialize().unwrap();
    task.complete_start_failure().unwrap();
    task.completion_request().unwrap();
    task.event_request().unwrap();
    assert_eq!(task.notification(), None);
    assert_eq!(task.state(), TaskState::S0);
    assert!(!task.completion_flag());
}

#[test]
fn backpressure_retains_previously_delivered_notifications() {
    let mut task = TaskRuntime::new();
    task.register(CompletionTarget::Operation, CompletionRegistration(17));
    let mut accepted = 0;
    loop {
        match task.reject_operation(InterfaceOperation::Configure) {
            Ok(()) => accepted += 1,
            Err(TaskError::NotificationCapacity) => break,
            other => panic!("unexpected result: {other:?}"),
        }
    }
    for _ in 0..accepted {
        assert_eq!(task.notification().unwrap().registration, CompletionRegistration(17));
    }
    assert_eq!(task.notification(), None);
    task.reject_operation(InterfaceOperation::Configure).unwrap();
    assert_eq!(task.notification().unwrap().status, CompletionStatus::NotInitialized);
}

#[test]
fn packet_boundary_rejects_wrong_lengths_and_out_of_range_fields() {
    assert_eq!(TaskPayload::parse(&[0; 199]), Err(PayloadError::Length));
    assert_eq!(TaskPayload::parse(&[0; 201]), Err(PayloadError::Length));
    let mut bytes = [0; 200];
    bytes[0..4].copy_from_slice(&0x12345678u32.to_le_bytes());
    let payload = TaskPayload::parse(&bytes).unwrap();
    assert_eq!(payload.word(0), Ok(0x12345678));
    assert_eq!(payload.halfword(0), Ok(0x5678));
    assert_eq!(payload.word(197), Err(PayloadError::Offset));
    assert_eq!(payload.word(usize::MAX), Err(PayloadError::Offset));
}

#[test]
fn initialization_admission_does_not_publish_a_completed_service() {
    use super::vendor_wireless_network::transport::socket_task::SocketTaskRuntime;
    let mut socket = SocketTaskRuntime::new();
    let mut task = TaskRuntime::new();
    task.register(CompletionTarget::Initialize, CompletionRegistration(7));
    task.initialize(&mut socket).unwrap();
    assert_eq!(task.state(), TaskState::S0);
    assert_eq!(task.phase(), TaskState::S0);
    assert_eq!(task.notification(), None);
    assert_eq!(socket.completion(), None);
    assert!(matches!(task.initialize(&mut socket), Err(TaskError::Busy)));
}

#[test]
fn queue_backpressure_returns_the_exclusive_packet_without_releasing_it() {
    use super::vendor_wireless_service_task::{TaskMessage, TaskMessageKind, TaskQueue};
    use super::vendor_wireless_service_task::discovery::{NetworkPacketPool, PacketStorage};
    let mut bytes = [0; 4];
    let mut packets = [PacketStorage::new(&mut bytes)];
    let mut pool = NetworkPacketPool::new(&mut packets);
    let packet = pool.insert(&[3, 5, 7]).unwrap();
    let mut storage: [Option<TaskMessage>; 0] = [];
    let mut queue = TaskQueue::new(&mut storage).unwrap();
    let message = TaskMessage::status(TaskMessageKind(0x6613), CompletionStatus::Success).with_packet(packet);
    let mut returned = queue.enqueue(message).unwrap_err().message;
    let packet = returned.packet.take().unwrap();
    assert_eq!(pool.bytes(&packet).unwrap(), &[3, 5, 7]);
    pool.release(packet).unwrap();
    assert!(pool.insert(&[11]).is_ok());
}

#[test]
fn queue_preserves_fifo_order_across_backpressure_and_wraparound() {
    use super::vendor_wireless_service_task::{TaskMessage, TaskMessageKind, TaskQueue};
    let mut storage: [Option<TaskMessage>; 2] = [None, None];
    let mut queue = TaskQueue::new(&mut storage).unwrap();
    for kind in [0x660b, 0xe60b] {
        queue.enqueue(TaskMessage::status(TaskMessageKind(kind), CompletionStatus::Success)).unwrap();
    }
    let returned = queue.enqueue(TaskMessage::status(TaskMessageKind(0x6609), CompletionStatus::Success)).unwrap_err().message;
    assert_eq!(queue.dequeue().unwrap().kind, TaskMessageKind(0x660b));
    queue.enqueue(returned).unwrap();
    assert_eq!(queue.dequeue().unwrap().kind, TaskMessageKind(0xe60b));
    assert_eq!(queue.dequeue().unwrap().kind, TaskMessageKind(0x6609));
    assert!(queue.is_empty());
}

#[test]
fn typed_interface_completions_keep_their_actual_queue_destinations() {
    use super::vendor_wireless_service_task::TaskMessage;
    use super::vendor_wireless_service_task::interface_modes::{InterfaceCompletion, InterfaceCompletionKind, InterfaceStatus};
    for (kind, code) in [(InterfaceCompletionKind::Initialized, 0x660c), (InterfaceCompletionKind::Configured, 0xe60b), (InterfaceCompletionKind::Reset, 0xe60b), (InterfaceCompletionKind::Started, 0x662a)] {
        let message = TaskMessage::from_interface_completion(InterfaceCompletion { kind, status: InterfaceStatus::ProtocolFailure });
        assert_eq!(message.kind.0, code);
        assert_eq!(message.payload.word(0).unwrap() as i32, -51);
    }
}
