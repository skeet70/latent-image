use crate::{commands::{ByteCount,ConfigurationUpload,UploadMessages,UploadPending,UploadProvider},service::UploadProgress,session::Tick,transport::SourceAction};
use crate::service_tests::{command,exchange,frame,open,with_service};
struct RefusedQueues;
impl UploadMessages for RefusedQueues{
    fn configuration(&mut self,_:ConfigurationUpload<'_>)->Result<(),i32>{Err(-1)}
    fn pending_transport(&mut self,_:UploadPending)->Result<(),i32>{Err(-1)}
}
#[test]
fn prepared_echo_upload_accumulates_chunks_before_publishing_the_sentinel_reply(){
    with_service(false,|service|{
        let sid=open(service);let mut prepare=command(0x760001,0,0);prepare[20..24].copy_from_slice(&8u32.to_le_bytes());prepare[24..28].copy_from_slice(&256u32.to_le_bytes());
        exchange(service,sid,&prepare[..32]);service.control(&frame(0xe0,0,4,sid),Tick(0)).unwrap();
        let mut queues=RefusedQueues;
        assert!(matches!(service.receive_payload(&[2,3,5,7],UploadProvider::Absent,&mut queues).unwrap(),UploadProgress::Pending{received:ByteCount(4),total:ByteCount(8),..}));
        match service.bulk_out_source().unwrap(){SourceAction::Transfer(span)=>assert_eq!((span.offset(),span.length()),(4,4)),other=>panic!("unexpected source: {other:?}")}
        assert!(matches!(service.receive_payload(&[11,13,17,19],UploadProvider::Absent,&mut queues).unwrap(),UploadProgress::Complete{..}));
        service.control(&frame(0xd1,0,128,sid),Tick(0)).unwrap();let reply=service.reply().unwrap();assert_eq!(u32::from_le_bytes(reply[..4].try_into().unwrap()),0x3fffff);
        assert_eq!(service.transport_mut().bulk_in_complete(crate::transport::CompletionStatus::Success),crate::transport::CompletionAction::SendStatus([0,0]));
        assert_eq!(service.transport_mut().bulk_in_complete(crate::transport::CompletionStatus::Success),crate::transport::CompletionAction::None);
        exchange(service,sid,&command(0x550001,256,0)[..32]);service.control(&frame(0xe1,0,8,sid),Tick(0)).unwrap();assert_eq!(service.payload().unwrap(),&[2,3,5,7,11,13,17,19]);
        assert_eq!(service.backend_mut().take_configuration_event(),None);
    });
}

#[test]
fn a_single_chunk_upload_publishes_a_new_reply_status_after_receive_status(){
    with_service(false,|service|{
        let sid=open(service);
        let mut prepare=command(0x760001,0,0);
        prepare[20..24].copy_from_slice(&4u32.to_le_bytes());
        exchange(service,sid,&prepare[..32]);
        service.control(&frame(0xe0,0,4,sid),Tick(0)).unwrap();
        let mut queues=RefusedQueues;
        assert!(matches!(service.receive_payload(&[2,3,5,7],UploadProvider::Absent,&mut queues).unwrap(),UploadProgress::Complete{..}));
        service.control(&frame(0xd1,0,128,sid),Tick(0)).unwrap();
        assert_eq!(service.reply().unwrap().len(),32);
        assert_eq!(service.transport_mut().bulk_in_complete(crate::transport::CompletionStatus::Success),crate::transport::CompletionAction::SendStatus([0,0]));
    });
}
