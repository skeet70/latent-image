use super::*;
use crate::lens_queue_runtime::physical_fixture::{setup,initialize,received,transmitted};
use std::sync::Mutex;
use std::vec::Vec;

#[derive(Default)]
struct Sender(Mutex<Vec<ControlMessage>>);
impl ControlSender for Sender {
    fn post(&self,message:ControlMessage)->Result<(),ControlError> {self.0.lock().unwrap().push(message);Ok(())}
}

#[test]
fn capability_exchange_writes_the_body_configuration_and_delivers_the_next_marker() {
    let (mut runtime,physical)=setup();
    initialize(&mut runtime);
    let runtime=RefCell::new(runtime);
    let mut queue=LensControlQueue::new(&runtime);
    queue.write_configuration(QueuePage(0),&[0x5a;128],DeviceCallback::CapabilityPage).unwrap();
    let wire=transmitted(&physical,131);
    assert_eq!(&wire[..2],&[0,0]);
    assert_eq!(&wire[2..130],&[0x5a;128]);
    assert_eq!(wire[130],0);
    let mut response=[0;131];response[0]=0xff;response[130]=0x5a;
    received(&mut runtime.borrow_mut(),&physical,&response);
    let publication=runtime.borrow_mut().pop_publication().unwrap();
    let sender=Sender::default();
    assert_eq!(queue.publication(publication,&sender),Ok(true));
    let messages=sender.0.lock().unwrap();
    assert!(matches!(&messages[..],[ControlMessage {opcode:ControlOpcode::Response,payload:ControlPayload::Response(ControlResponse {kind:ResponseKind::Reply24,status:CompletionStatus::SUCCESS,data,..})}] if data[..2]==[0,0xff]));
}

#[test]
fn canceled_calibration_read_delivers_failure_without_a_second_acknowledgement() {
    let (mut runtime,_physical)=setup();
    initialize(&mut runtime);
    let runtime=RefCell::new(runtime);
    let mut queue=LensControlQueue::new(&runtime);
    queue.accept_page(false,DeviceCallback::Response(ResponseKind::Reply19)).unwrap();
    runtime.borrow_mut().cancel().unwrap();
    let publication=runtime.borrow_mut().pop_publication().unwrap();
    let sender=Sender::default();
    assert_eq!(queue.publication(publication,&sender),Ok(true));
    assert!(matches!(&sender.0.lock().unwrap()[..],[ControlMessage {payload:ControlPayload::Response(ControlResponse {kind:ResponseKind::Reply19,status:CompletionStatus(2),..}),..}]));
}
