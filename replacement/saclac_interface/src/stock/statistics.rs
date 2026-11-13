#![forbid(unsafe_code)]

//! X-M1 1.50 payloads of the requests saclac posts to statistics (queue 48).

use super::Encoded;
use crate::statistics::StatisticsPayload;
use crate::StatisticsOperation;

/// Encodes each request as its sender 0x103a09c, 0x103a13c, 0x103a200 or 0x103b064 stores it.
pub fn payload(payload: &StatisticsPayload) -> (StatisticsOperation, Encoded) {
    use StatisticsPayload as P;
    match *payload {
        P::Request0b { value } => {
            let mut message = Encoded::new(4);
            message.word(0, value);
            (StatisticsOperation::Request0b, message)
        }
        P::Request0e { record, callback } => {
            let mut message = Encoded::new(32);
            message.put(0, &record.0);
            message.word(28, callback.0);
            (StatisticsOperation::Request0e, message)
        }
        P::Request0f { callback } => {
            let mut message = Encoded::new(4);
            message.word(0, callback.0);
            (StatisticsOperation::Request0f, message)
        }
        P::Request43 => (StatisticsOperation::Request43, Encoded::new(0)),
    }
}
