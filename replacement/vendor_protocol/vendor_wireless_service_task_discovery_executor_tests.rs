use super::executor::*;
use super::packet::{OperationCode,TransactionId};

#[test]
fn partial_acknowledgement_preserves_unsent_suffix() {
    let mut output = PtpOutput::new();
    output.send_data(OperationCode(0x1004),TransactionId(7),&[1,2,3,4]).unwrap();
    assert_eq!(output.queued_bytes(), &[16,0,0,0,2,0,4,0x10,7,0,0,0,1,2,3,4]);
    output.acknowledge(5).unwrap();
    assert_eq!(output.queued_bytes(), &[0,4,0x10,7,0,0,0,1,2,3,4]);
    assert!(matches!(output.acknowledge(12),Err(PtpError::InvalidLength)));
    assert_eq!(output.queued_bytes().len(),11);
    output.acknowledge(11).unwrap();
    assert!(output.is_empty());
}

#[test]
fn dataset_stream_does_not_repeat_header_after_chunk_ack() {
    let mut output = PtpOutput::new();
    output.begin_data(OperationCode(0x1007),TransactionId(3),3000).unwrap();
    assert_eq!(output.write_data(&[5;3000]).unwrap(),2036);
    assert_eq!(output.queued_bytes().len(),2048);
    assert_eq!(output.data_remaining(),964);
    output.acknowledge(2048).unwrap();
    assert_eq!(output.write_data(&[6;964]).unwrap(),964);
    assert_eq!(output.queued_bytes(), &[6;964]);
    assert_eq!(output.data_remaining(),0);
    output.acknowledge(964).unwrap();
    output.send_response(OperationReply::new(ResponseCode(0x2001)),TransactionId(3)).unwrap();
    assert_eq!(output.queued_bytes(), &[12,0,0,0,3,0,1,0x20,3,0,0,0]);
}

#[test]
fn response_cannot_overtake_dataset_and_discard_cancels_all_output() {
    let mut output = PtpOutput::new();
    output.begin_data(OperationCode(0x1009),TransactionId(2),4).unwrap();
    output.acknowledge(12).unwrap();
    assert!(matches!(output.send_response(OperationReply::new(ResponseCode(0x2001)),TransactionId(2)),Err(PtpError::OutputFull)));
    output.discard();
    output.send_response(OperationReply::new(ResponseCode(0x201f)),TransactionId(2)).unwrap();
    assert_eq!(output.queued_bytes(), &[12,0,0,0,3,0,0x1f,0x20,2,0,0,0]);
}
