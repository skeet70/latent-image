use super::*;
use core::future::Future;
use core::task::Context;
use std::{collections::VecDeque,sync::Mutex};

#[derive(Default)]
struct Queue(Mutex<VecDeque<ControlMessage>>);
impl ControlSender for Queue {
    fn post(&self,message:ControlMessage)->Result<(),ControlError> {self.0.lock().unwrap().push_back(message);Ok(())}
}
impl Queue {
    fn snapshot_callback(&self)->CallbackToken {
        match self.0.lock().unwrap().pop_front().unwrap().payload {
            ControlPayload::Snapshot {callback,..}=>callback,
            _=>panic!("snapshot request"),
        }
    }
}

#[test]
fn dropped_snapshot_cannot_complete_a_later_ticket() {
    let queue=Queue::default();
    let mailbox=CompletionMailbox::new();
    let publications=ControlPublications::new();
    let readiness=SharedReadiness::new();
    let client=Task99Client::new(&queue,&mailbox,&publications,&readiness);
    let abandoned=client.begin_snapshot(ObserverKind::NineBytes).unwrap();
    let abandoned_callback=queue.snapshot_callback();
    assert_eq!(abandoned.try_complete(),Ok(None));
    drop(abandoned);
    let pending=client.begin_snapshot(ObserverKind::NineBytes).unwrap();
    let callback=queue.snapshot_callback();
    assert!(!mailbox.complete(abandoned_callback,CompletionStatus::SUCCESS,&[9;9]));
    assert_eq!(pending.try_complete(),Ok(None));
    assert!(mailbox.complete(callback,CompletionStatus::SUCCESS,&[1,2,3,4,5,6,7,8,9]));
    assert_eq!(pending.try_complete(),Ok(Some(SnapshotResult {status:CompletionStatus::SUCCESS,data:SnapshotData::NineBytes([1,2,3,4,5,6,7,8,9])})));
}

#[test]
fn asynchronous_snapshot_waits_for_completion_delivery() {
    let queue=Queue::default();
    let mailbox=CompletionMailbox::new();
    let publications=ControlPublications::new();
    let readiness=SharedReadiness::new();
    let client=Task99Client::new(&queue,&mailbox,&publications,&readiness);
    let mut request=std::pin::pin!(client.snapshot(ObserverKind::ThreeBytes));
    let mut context=Context::from_waker(core::task::Waker::noop());
    assert_eq!(request.as_mut().poll(&mut context),Poll::Pending);
    let callback=queue.snapshot_callback();
    assert!(mailbox.complete(callback,CompletionStatus::SUCCESS,&[4,5,6]));
    assert_eq!(request.as_mut().poll(&mut context),Poll::Ready(Ok(SnapshotResult {status:CompletionStatus::SUCCESS,data:SnapshotData::ThreeBytes([4,5,6])})));
}

#[test]
fn snapshot_ticket_preserves_driver_failure_status() {
    let queue=Queue::default();
    let mailbox=CompletionMailbox::new();
    let publications=ControlPublications::new();
    let readiness=SharedReadiness::new();
    let client=Task99Client::new(&queue,&mailbox,&publications,&readiness);
    let pending=client.begin_snapshot(ObserverKind::NineBytes).unwrap();
    let callback=queue.snapshot_callback();
    assert!(mailbox.complete(callback,CompletionStatus(2),&[]));
    assert!(matches!(pending.try_complete(),Ok(Some(SnapshotResult {status:CompletionStatus(2),..}))));
}
