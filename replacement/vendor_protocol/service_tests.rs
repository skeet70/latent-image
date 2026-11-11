use crate::{backend::{ConfigImages,DebugMemory,CONFIG_BYTES},backend_adapter::{CommandBackend,DebugRing,DEBUG_RING_BYTES},commands::{Backend,ConfigNamespace},configuration_rules::STOCK_RULES,debug_events::ApplicationEvent,service::Service,session::{FrameError,Tick},transport::PublishedRegion};
use std::vec;

pub(crate) fn frame(op:u8,offset:u32,length:u32,token:u16)->[u8;12]{
    let mut frame=[0;12];frame[0]=op;
    frame[2..6].copy_from_slice(&offset.to_be_bytes());frame[6..10].copy_from_slice(&length.to_be_bytes());frame[10..12].copy_from_slice(&token.to_be_bytes());frame
}
pub(crate) fn command(opcode:u32,function:u32,length:u32)->[u8;512]{
    let mut request=[0;512];request[..4].copy_from_slice(&opcode.to_le_bytes());request[16..20].copy_from_slice(&function.to_le_bytes());request[32..36].copy_from_slice(&length.to_le_bytes());request
}
pub(crate) fn with_service(gate:bool,action:impl FnOnce(&mut Service<'_,CommandBackend<'_>>)){
    let first=vec![0u8;0x10000];let second=vec![0u8;0x410000];let diagnostic=vec![0u8;0x8000];let encoded_diagnostic=vec![0u8;0x8000];let mut encoded=vec![0u8;CONFIG_BYTES as usize];encoded[0xf7]=u8::from(gate);
    let mut memory=[0u8;64];let ring=[0u8;DEBUG_RING_BYTES];let mut workspace=vec![0u8;0x10000];
    let configuration=ConfigImages::adopt(&first,&second,&diagnostic,&mut encoded,&encoded_diagnostic);
    let backend=CommandBackend::new(configuration,DebugMemory::adopt(0x41020000,&mut memory).unwrap(),DebugRing::adopt(&ring,0,0,false,0).unwrap(),&STOCK_RULES).unwrap();
    let mut request=[0;512];let mut reply=[0;512];let mut control=[0;256];
    let aperture=crate::transport::PublishedAperture::publish(PublishedRegion::new(0x41040000,0x10000).unwrap(),&mut workspace).unwrap();
    let buffers=crate::service::ServiceBuffers::new((&mut request,PublishedRegion::new(0x41024000,512).unwrap()),(&mut reply,PublishedRegion::new(0x41025000,512).unwrap()),(&mut control,PublishedRegion::new(0x41026000,256).unwrap()),aperture).unwrap();
    let mut service=Service::new(buffers,backend,crate::transport::ReceiveProgramming::new(0,0,0));action(&mut service);
}
pub(crate) fn open(service:&mut Service<'_,CommandBackend<'_>>)->u16{
    service.control(&frame(0xc0,0,8,0),Tick(0)).unwrap();let bytes=service.reply().unwrap();u16::from_le_bytes([bytes[0],bytes[1]])
}
pub(crate) fn exchange(service:&mut Service<'_,CommandBackend<'_>>,sid:u16,request:&[u8])->std::vec::Vec<u8>{
    service.control(&frame(0xd0,0,request.len() as u32,sid),Tick(0)).unwrap();service.receive_command(request).unwrap();
    service.control(&frame(0xd1,0,512,sid),Tick(0)).unwrap();service.reply().unwrap().to_vec()
}
#[test]
fn command_reply_and_bulk_download_are_joined_to_actual_generated_bytes(){
    with_service(false,|service|{
        let sid=open(service);let request=command(0x550001,0,0);let reply=exchange(service,sid,&request[..32]);
        assert_eq!(&reply[4..12],&[0;8]);assert_eq!(u32::from_le_bytes(reply[20..24].try_into().unwrap()),65536);
        service.control(&frame(0xe1,0xff00,256,sid),Tick(0)).unwrap();assert_eq!(service.payload().unwrap(),&(0..=255).collect::<std::vec::Vec<u8>>());
        assert!(service.control(&frame(0xe1,0xffff,2,sid),Tick(0)).is_err());
        assert!(service.payload().is_err());
        service.control(&frame(0xc4,0,2,sid),Tick(0)).unwrap();assert_eq!(service.reply().unwrap(),&sid.to_le_bytes());
        assert_eq!(service.backend_mut().take_application_event(),None);
    });
}
#[test]
fn power_off_is_a_gated_application_message_and_close_does_not_emit_one(){
    for gate in [false,true]{with_service(gate,|service|{
        let sid=open(service);if gate{service.backend_mut().register_application_executor().unwrap();}
        let mut request=command(0x200001,5,10);request[36..46].copy_from_slice(b"POWER OFF\0");
        let reply=exchange(service,sid,&request[..46]);
        let expected=if gate{(0u32,0u32)}else{(1,3)};
        assert_eq!((u32::from_le_bytes(reply[4..8].try_into().unwrap()),u32::from_le_bytes(reply[8..12].try_into().unwrap())),expected);
        assert_eq!(service.backend_mut().take_application_event(),if gate{Some(ApplicationEvent::PowerOffRequested)}else{None});
        service.control(&frame(0xc4,0,2,sid),Tick(0)).unwrap();assert_eq!(service.backend_mut().take_application_event(),None);
    });}
}
#[test]
fn configuration_write_is_visible_on_following_read_and_read_does_not_emit_storage_work(){
    with_service(false,|service|{
        let sid=open(service);let mut write=command(0x1a0002,3,3);write[20..24].copy_from_slice(&1u32.to_le_bytes());write[28..32].copy_from_slice(&0x90u32.to_le_bytes());write[36..39].copy_from_slice(&[2,3,5]);
        let reply=exchange(service,sid,&write[..39]);assert_eq!(&reply[4..12],&[0;8]);
        let mut read=command(0x1a0002,2,3);read[20..24].copy_from_slice(&1u32.to_le_bytes());read[28..32].copy_from_slice(&0x90u32.to_le_bytes());
        let reply=exchange(service,sid,&read[..36]);assert_eq!(&reply[36..39],&[2,3,5]);assert_eq!(service.backend_mut().take_configuration_event(),None);
        let mut observed=[0;3];assert_eq!(service.backend_mut().config_read(ConfigNamespace::Encoded,0x90,&mut observed),crate::commands::BackendStatus::Complete);assert_eq!(observed,[2,3,5]);
    });
}
#[test]
fn no_reply_is_published_before_the_command_consumer_and_d1_admission(){
    with_service(false,|service|{
        let sid=open(service);service.control(&frame(0xd0,0,32,sid),Tick(0)).unwrap();
        assert!(matches!(service.reply(),Err(crate::service::ServiceError::Frame(FrameError::ReplyNotReady))));
        service.receive_command(&command(0x1a0000,0,0)[..32]).unwrap();
        assert!(matches!(service.reply(),Err(crate::service::ServiceError::Frame(FrameError::ReplyNotReady))));
        service.control(&frame(0xd1,0,128,sid),Tick(0)).unwrap();assert_eq!(service.reply().unwrap().len(),32);
    });
}

#[test]
fn zero_length_control_replies_are_bulk_transfers_not_empty_payload_spans(){
    for opcode in [0x03,0xc0,0xc2,0xc4]{
        with_service(false,|service|{
            let sid=if matches!(opcode,0xc2|0xc4){open(service)}else{0};
            service.control(&frame(opcode,0,0,sid),Tick(0)).unwrap();
            match service.bulk_in_source().unwrap(){
                crate::service::BulkIn::EmptyReply(region)=>assert_eq!(region.base(),0x41026000),
                _=>panic!("zero-length control reply was not published"),
            }
            assert!(matches!(service.bulk_in_source().unwrap(),crate::service::BulkIn::NotReady));
            let status=if opcode==0x03{[0,0]}else{[0x21,0]};
            assert_eq!(service.transport_mut().bulk_in_complete(crate::transport::CompletionStatus::Success),crate::transport::CompletionAction::SendStatus(status));
        });
    }
}

#[test]
fn inquiry_preserves_the_latest_application_reply_not_an_older_control_reply(){
    with_service(false,|service|{
        let sid=open(service);
        exchange(service,sid,&command(0x1a0000,0,0)[..32]);
        service.control(&frame(0x03,8<<8,0,0),Tick(0)).unwrap();
        assert_eq!(service.reply().unwrap(),&[0x70,0,0,0,0,0,0,10]);
    });
}

#[test]
fn a_later_provider_completion_cannot_republish_an_earlier_payload(){
    use crate::commands::{CommandError,CompletionCode,CompletionDisposition,CompletionError,ControllerState,DiagnosticLayout,ImageDescriptor,ProviderRegions,VendorCompletion};
    use crate::controller::{CompletionHandle,ControllerError,QueueController,RequestHandle};
    for deferred in [false,true]{
        with_service(false,|service|{
            let sid=open(service);
            service.control(&frame(0xd0,0,32,sid),Tick(0)).unwrap();
            service.receive_command(&command(0x450006,0x2b3,0)[..32]).unwrap();
            let regions=ProviderRegions{primary_log:None,secondary_log:None,image:None,diagnostic:None,descriptor_image:None,device_info:None,diagnostic_layout:DiagnosticLayout::Ordinary};
            service.provide(ImageDescriptor::new(0).unwrap(),&regions).unwrap();
            service.control(&frame(0xd1,0,128,sid),Tick(0)).unwrap();
            assert_eq!(&service.reply().unwrap()[20..24],&[0;4]);
            service.control(&frame(0xd0,0,32,sid),Tick(0)).unwrap();
            service.receive_command(&command(0x450006,0x2b2,0)[..32]).unwrap();
            let mut controller=QueueController::new(ControllerState::ActiveVendor,CompletionHandle(0),RequestHandle(3));
            let completed=VendorCompletion::Command{status:CompletionCode(0)};
            if deferred{assert_eq!(service.consume_queue_completion(&mut controller,completed,None).unwrap(),CompletionDisposition::Deferred);}
            controller.transition(ControllerState::PendingVendor).unwrap();
            let outcome=service.consume_queue_completion(&mut controller,completed,None);
            assert!(matches!(outcome,Err(crate::service::ServiceError::Controller(ControllerError::Completion(CompletionError::Command(CommandError::ProviderPayloadRequired))))));
            service.control(&frame(0xd1,0,128,sid),Tick(0)).unwrap();
            assert!(matches!(service.reply(),Err(crate::service::ServiceError::Frame(FrameError::ReplyNotReady))));
        });
    }
}

#[test]
fn a_late_configuration_completion_keeps_the_intervening_control_reply_selected(){
    use crate::commands::{CompletionCode,CompletionDisposition,ControllerState,VendorCompletion};
    use crate::controller::{CompletionHandle,QueueController,RequestHandle};
    for deferred in [false,true]{
        with_service(false,|service|{
            let sid=open(service);
            let mut request=command(0x1a0002,3,1);
            request[20..24].copy_from_slice(&1u32.to_le_bytes());
            request[28..32].copy_from_slice(&0x80u32.to_le_bytes());
            request[36]=1;
            service.control(&frame(0xd0,0,37,sid),Tick(0)).unwrap();
            service.receive_command(&request[..37]).unwrap();
            let mut controller=QueueController::new(ControllerState::ActiveVendor,CompletionHandle(0),RequestHandle(3));
            let completed=VendorCompletion::Command{status:CompletionCode(0)};
            if deferred{assert_eq!(service.consume_queue_completion(&mut controller,completed,None).unwrap(),CompletionDisposition::Deferred);}
            service.control(&frame(0xc2,0,2,sid),Tick(0)).unwrap();
            assert_eq!(service.reply().unwrap(),&sid.to_le_bytes());
            controller.transition(ControllerState::PendingVendor).unwrap();
            assert_eq!(service.consume_queue_completion(&mut controller,completed,None).unwrap(),CompletionDisposition::Command{ready:true});
            service.control(&frame(0xd1,0,128,sid),Tick(0)).unwrap();
            assert_eq!(service.reply().unwrap(),&sid.to_le_bytes());
        });
    }
}

#[test]
fn a_new_connection_closes_the_session_and_drops_the_pending_command(){
    use crate::commands::{CompletionCode,CompletionDisposition,ControllerState,ServiceState,VendorCompletion};
    use crate::controller::{CompletionHandle,QueueController,RequestHandle};
    with_service(false,|service|{
        let sid=open(service);
        let mut request=command(0x1a0002,3,1);
        request[20..24].copy_from_slice(&1u32.to_le_bytes());
        request[28..32].copy_from_slice(&0x80u32.to_le_bytes());
        request[36]=1;
        service.control(&frame(0xd0,0,37,sid),Tick(0)).unwrap();
        service.receive_command(&request[..37]).unwrap();
        assert!(service.session().active());
        service.begin_connection();
        service.open_connection();
        assert!(!service.session().active());
        assert!(matches!(service.reply(),Err(crate::service::ServiceError::Frame(FrameError::ReplyNotReady))));
        assert_eq!(service.commands_mut().service_state,ServiceState::Normal);
        let mut controller=QueueController::new(ControllerState::PendingVendor,CompletionHandle(0),RequestHandle(3));
        let completed=VendorCompletion::Command{status:CompletionCode(0)};
        assert_eq!(service.consume_queue_completion(&mut controller,completed,None).unwrap(),CompletionDisposition::Ignored);
        assert_ne!(open(service),0);
    });
}
