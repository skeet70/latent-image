use super::{FirmwareVersion, Query, QueryError, QueryReply};
use crate::vendor_wireless_network::MacAddress;

#[test]
fn query_requests_use_get_semantics_and_caller_sequence() {
    let mut packet = [0xaa; 28];
    assert_eq!(Query::Address.encode(0xfe, &mut packet), 20);
    assert_eq!(&packet[..8], &[20, 0, 0, 0x50, 0xc1, 0, 0xfe, 0]);
    assert_eq!(&packet[8..20], &[0; 12]);
    assert_eq!(Query::Capabilities.encode(0xff, &mut packet), 28);
    assert_eq!(&packet[..8], &[28, 0, 0, 0x50, 0x80, 0, 0xff, 0]);
}

#[test]
fn address_reply_requires_event_packet_and_copies_six_bytes() {
    let mut packet = [0; 20];
    packet[2] = 1;
    packet[4] = 9;
    packet[8..14].copy_from_slice(&[2, 3, 4, 5, 6, 7]);
    assert_eq!(Query::Address.parse(&packet), Ok(QueryReply::Address(MacAddress([2, 3, 4, 5, 6, 7]))));
    assert_eq!(Query::Address.parse(&packet[..13]), Err(QueryError::Truncated));
    packet[4] = 2;
    assert_eq!(Query::Address.parse(&packet), Err(QueryError::WrongEvent));
    packet[2] = 0;
    assert_eq!(Query::Address.parse(&packet), Err(QueryError::WrongPacketType));
}

#[test]
fn capabilities_reply_preserves_versions_flags_and_both_feature_words() {
    let mut packet = [0; 28];
    packet[2] = 1;
    packet[4] = 2;
    packet[8..18].copy_from_slice(&[0x31, 0x59, 7, 1, 9, 0, 1, 2, 1, 1]);
    packet[20..24].copy_from_slice(&0x1122_3344u32.to_le_bytes());
    packet[24..28].copy_from_slice(&0x5566_7788u32.to_le_bytes());
    let QueryReply::Capabilities(capabilities) = Query::Capabilities.parse(&packet).unwrap() else { panic!() };
    assert_eq!(capabilities.product, 0x5931);
    assert_eq!(capabilities.version, FirmwareVersion(0x107));
    assert_eq!(capabilities.build, 9);
    assert!(capabilities.eeprom_used);
    assert!(capabilities.preserve_invalid_address);
    assert_eq!(capabilities.compilation, 0x1122_3344);
    assert_eq!(capabilities.features, 0x5566_7788);
}
