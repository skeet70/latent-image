#![forbid(unsafe_code)]

//! X-M1 1.50 payload bytes of completions 0x61 and 0x97..0x9a as task 43 receives them: the
//! payload starts at +0x10 of the receive buffer 0x169d370.

use crate::capture_results::{FirstResult, FourthResult, ReadoutPayload, SecondResult, ThirdResult, FIRST_RESULT_BYTES, SECOND_RESULT_BYTES};

fn word(payload: &[u8], offset: usize) -> Option<u32> { Some(u32::from_le_bytes(payload.get(offset..offset + 4)?.try_into().ok()?)) }

fn half(payload: &[u8], offset: usize) -> Option<u16> { Some(u16::from_le_bytes(payload.get(offset..offset + 2)?.try_into().ok()?)) }

/// 0xf2e7e0 reads byte +4, 0xf2e824 byte +5, 0xf2e7e8 and 0xf2e7f0 words +0x1c and +0x20.
pub fn readout(payload: &[u8]) -> Option<ReadoutPayload> {
    Some(ReadoutPayload { source: *payload.get(4)?, variant: *payload.get(5)?, mode: word(payload, 0x1c)?, value: word(payload, 0x20)? })
}

/// 0xf2edd8..0xf2ede4 copy 12 bytes.
pub fn first_result(payload: &[u8]) -> Option<FirstResult> { Some(FirstResult(payload.get(..FIRST_RESULT_BYTES)?.try_into().ok()?)) }

/// 0xf2ebdc..0xf2ebf8 copy 48 bytes.
pub fn second_result(payload: &[u8]) -> Option<SecondResult> { Some(SecondResult(payload.get(..SECOND_RESULT_BYTES)?.try_into().ok()?)) }

/// 0xf2ed28 reads word +0.
pub fn third_result(payload: &[u8]) -> Option<ThirdResult> { word(payload, 0).map(ThirdResult) }

/// 0xf2eed4, 0xf2eee0 and 0xf2eeec read byte +0 and halfwords +2 and +4.
pub fn fourth_result(payload: &[u8]) -> Option<FourthResult> {
    Some(FourthResult { first: *payload.first()?, second: half(payload, 2)?, third: half(payload, 4)? })
}
