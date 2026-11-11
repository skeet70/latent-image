use crate::session::{ControlAction,ControlFrame,ControlStatus,FrameError,Session,StatusDelivery,Tick};
use crate::transport::PublishedRegion;

fn frame(op:u8,offset:u32,length:u32,token:u16)->ControlFrame {
    let mut bytes=[0;12];
    bytes[0]=op;
    bytes[2..6].copy_from_slice(&offset.to_be_bytes());
    bytes[6..10].copy_from_slice(&length.to_be_bytes());
    bytes[10..12].copy_from_slice(&token.to_be_bytes());
    ControlFrame::parse(&bytes).unwrap()
}
fn open(session:&mut Session<'_>,tick:u32)->u16 {
    let outcome=session.control(frame(0xc0,0,8,0),Tick(tick),None).unwrap();
    assert_eq!(outcome.status,StatusDelivery::Accept(ControlStatus::Success));
    let reply=session.take_reply(&[]).unwrap();
    assert_eq!(&reply[2..],&[0,0,0,0,1,0]);
    u16::from_le_bytes([reply[0],reply[1]])
}

#[test]
fn open_allocates_nonzero_thirteen_bit_sid_from_counter_then_advances_without_reseeding() {
    for tick in [0,1,0x1ffe,0x1fff,0xffff,u32::MAX] {
        let mut buffer=[0;256];
        let mut session=Session::new(&mut buffer);
        let sid=open(&mut session,tick);
        let raw=(tick as u16).wrapping_add(1)&0x1fff;
        assert_eq!(sid,if raw==0 {1}else{raw});
        session.control(frame(0xc4,0,2,sid),Tick(0),None).unwrap();
        let next=open(&mut session,0x5555);
        assert_eq!(next,if sid==0x1fff {1}else{sid+1});
    }
}

#[test]
fn open_errors_preserve_session_and_still_prepare_fixed_reply() {
    let mut buffer=[0;256];
    let mut session=Session::new(&mut buffer);
    let bad=session.control(frame(0xc0,0,7,9),Tick(0),None).unwrap();
    assert_eq!(bad.status,StatusDelivery::Accept(ControlStatus::InvalidSession));
    assert!(!session.active());
    assert_eq!(session.take_reply(&[]).unwrap(),&[0,0,0,0,0,0,1]);
    let sid=open(&mut session,7);
    let bad=session.control(frame(0xc0,0,8,0),Tick(0),None).unwrap();
    assert_eq!(bad.status,StatusDelivery::Accept(ControlStatus::AlreadyActive));
    assert_eq!(session.id().unwrap().get(),sid);
}

#[test]
fn failed_validate_arms_a_zero_token_reply_without_opening_a_session() {
    let mut buffer=[0;256];
    let mut session=Session::new(&mut buffer);
    let result=session.control(frame(0xc2,0,2,0),Tick(0),None).unwrap();
    assert_eq!(result.status,StatusDelivery::Accept(ControlStatus::InvalidSession));
    assert_eq!(session.take_reply(&[]).unwrap(),&[0,0]);
    assert!(!session.active());
    let sid=open(&mut session,12);
    let result=session.control(frame(0xc2,0,3,sid^1),Tick(0),None).unwrap();
    assert_eq!(result.status,StatusDelivery::Accept(ControlStatus::InvalidLength));
    assert_eq!(session.take_reply(&[]).unwrap(),&[0,0]);
    assert_eq!(session.requested_length(),3);
    let result=session.control(frame(0xc2,0,2,sid),Tick(0),None).unwrap();
    assert_eq!(result.status,StatusDelivery::Accept(ControlStatus::Success));
    assert_eq!(session.take_reply(&[]).unwrap(),&sid.to_le_bytes());
}

#[test]
fn close_is_idempotent_and_checks_sid_before_length() {
    let mut buffer=[0;256];
    let mut session=Session::new(&mut buffer);
    let sid=open(&mut session,21);
    let bad=session.control(frame(0xc4,0,3,sid^1),Tick(0),None).unwrap();
    assert_eq!(bad.status,StatusDelivery::Accept(ControlStatus::InvalidSession));
    assert!(session.active());
    for _ in 0..2 {
        let closed=session.control(frame(0xc4,0,2,sid),Tick(0),None).unwrap();
        assert_eq!(closed.status,StatusDelivery::Accept(ControlStatus::Success));
        assert!(!session.active());
        assert_eq!(session.take_reply(&[]).unwrap(),&sid.to_le_bytes());
    }
}

#[test]
fn command_admission_and_reply_use_published_ready_bytes() {
    let mut buffer=[0;256];
    let mut session=Session::new(&mut buffer);
    let bad=session.control(frame(0xd0,0,513,4),Tick(0),None).unwrap();
    assert_eq!(bad.status,StatusDelivery::Reject(ControlStatus::InvalidSession));
    let sid=open(&mut session,0);
    let bad=session.control(frame(0xd0,0,513,sid),Tick(0),None).unwrap();
    assert_eq!(bad.status,StatusDelivery::Reject(ControlStatus::InvalidLength));
    let accepted=session.control(frame(0xd0,0,32,sid),Tick(0),None).unwrap();
    assert_eq!(accepted.action,ControlAction::ReceiveCommand{length:32});
    assert_eq!(session.take_reply(&[]),Err(FrameError::ReplyNotReady));
    session.publish_reply(4,false).unwrap();
    session.control(frame(0xd1,0,128,sid),Tick(0),None).unwrap();
    assert_eq!(session.take_reply(&[2,3,5,7]),Err(FrameError::ReplyNotReady));
    session.publish_reply(4,true).unwrap();
    assert_eq!(session.take_reply(&[2,3,5,7]).unwrap(),&[2,3,5,7]);
    session.control(frame(0xd1,0,2,sid),Tick(0),None).unwrap();
    assert_eq!(session.take_reply(&[2,3,5,7]),Err(FrameError::ReplyNotReady));
    session.publish_reply(4,true).unwrap();
    assert_eq!(session.take_reply(&[2,3,5,7]).unwrap(),&[2,3]);
}

#[test]
fn download_requires_a_published_nonempty_checked_span_and_next_command_clears_it() {
    let mut buffer=[0;256];
    let mut session=Session::new(&mut buffer);
    let sid=open(&mut session,8);
    let region=PublishedRegion::new(0x41000000,16).unwrap();
    for (offset,length) in [(16,1),(15,2),(u32::MAX,2),(1,u32::MAX)] {
        assert_eq!(session.control(frame(0xe1,offset,length,sid),Tick(0),Some(region)),Err(FrameError::InvalidPublishedSpan));
        assert_eq!(session.payload_span(),None);
    }
    let out=session.control(frame(0xe1,15,1,sid),Tick(0),Some(region)).unwrap();
    match out.action {ControlAction::SendPayload(span)=>assert_eq!((span.address(),span.length()),(0x4100000f,1)),_=>panic!("missing payload span")};
    session.control(frame(0xc2,0,2,sid),Tick(0),Some(region)).unwrap();
    assert_eq!(session.payload_span(),None);
    let zero=session.control(frame(0xe1,16,0,sid),Tick(0),Some(region)).unwrap();
    assert_eq!(zero.status,StatusDelivery::Reject(ControlStatus::InvalidLength));
}

#[test]
fn malformed_control_never_enters_the_session_machine() {
    assert_eq!(ControlFrame::parse(&[0xc0;11]),Err(FrameError::Truncated));
    let mut bytes=[0;12];bytes[1]=0x20;
    assert_eq!(ControlFrame::parse(&bytes),Err(FrameError::ReservedFlags));
}
