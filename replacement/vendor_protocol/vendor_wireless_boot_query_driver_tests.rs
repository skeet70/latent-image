use super::{QueryDriver, QueryFailure};
use crate::sd_controller::{Buffer, Completion, PhysicalResult, VendorCode};
use crate::vendor_wireless_boot_query::{Query, QueryReply};
use crate::vendor_wireless_network::MacAddress;
use crate::vendor_wireless_power::RadioTick;

fn physical() -> Completion { Completion { code: VendorCode::Success, physical: PhysicalResult::Completed, transferred: 0, status: 0, response: 0 } }

#[test]
fn queued_query_is_not_complete_until_event_fifo_is_physically_read() {
    let mut query = QueryDriver::new(Query::Address, 7);
    let mut register = [0; 4];
    let mut packet = [0; 28];
    let request = query.request(RadioTick(0), &mut register, &mut packet).unwrap();
    assert_eq!(request.argument.wire(), 0x9000_5814);
    drop(request);
    assert_eq!(query.outcome(), None);
    assert!(query.request(RadioTick(0), &mut register, &mut packet).is_none());
    assert!(query.receive(physical(), register, &packet, RadioTick(0), false));
    let request = query.request(RadioTick(0), &mut register, &mut packet).unwrap();
    assert_eq!(request.argument.wire(), 0x1000_a004);
    drop(request);
    register = (20u32 << 16).to_le_bytes();
    query.receive(physical(), register, &packet, RadioTick(1), false);
    let request = query.request(RadioTick(1), &mut register, &mut packet).unwrap();
    assert_eq!(request.argument.wire(), 0x1000_6814);
    let Buffer::Fifo(bytes) = request.buffer else { panic!() };
    bytes.fill(0);
    bytes[2] = 1;
    bytes[4] = 9;
    bytes[8..14].copy_from_slice(&[2, 4, 6, 8, 10, 12]);
    query.receive(physical(), register, &packet, RadioTick(2), false);
    assert_eq!(query.outcome(), Some(Ok(QueryReply::Address(MacAddress([2, 4, 6, 8, 10, 12])))));
    assert!(!query.receive(physical(), register, &packet, RadioTick(3), false));
}

#[test]
fn absent_reply_times_out_and_cancel_never_publishes_success() {
    for cancelled in [false, true] {
        let mut query = QueryDriver::new(Query::Capabilities, 1);
        let mut register = [0; 4];
        let mut packet = [0; 28];
        drop(query.request(RadioTick(0), &mut register, &mut packet).unwrap());
        query.receive(physical(), register, &packet, RadioTick(0), false);
        drop(query.request(RadioTick(1001), &mut register, &mut packet).unwrap());
        query.receive(physical(), register, &packet, RadioTick(1001), cancelled);
        assert_eq!(query.outcome(), Some(Err(if cancelled { QueryFailure::Cancelled } else { QueryFailure::Timeout })));
    }
}

#[test]
fn malformed_length_and_physical_failure_cannot_become_query_replies() {
    let mut query = QueryDriver::new(Query::Address, 1);
    let mut register = [0; 4];
    let mut packet = [0; 28];
    drop(query.request(RadioTick(0), &mut register, &mut packet).unwrap());
    let mut failed = physical();
    failed.physical = PhysicalResult::Unconfirmed;
    query.receive(failed, register, &packet, RadioTick(0), false);
    assert_eq!(query.outcome(), Some(Err(QueryFailure::Physical)));
    let mut query = QueryDriver::new(Query::Address, 2);
    drop(query.request(RadioTick(0), &mut register, &mut packet).unwrap());
    query.receive(physical(), register, &packet, RadioTick(0), false);
    drop(query.request(RadioTick(1), &mut register, &mut packet).unwrap());
    query.receive(physical(), (21u32 << 16).to_le_bytes(), &packet, RadioTick(1), false);
    assert_eq!(query.outcome(), Some(Err(QueryFailure::Oversized)));
}
