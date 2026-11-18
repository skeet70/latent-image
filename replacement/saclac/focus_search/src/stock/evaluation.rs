#![forbid(unsafe_code)]

//! The evaluation a search's calc completion delivers.

use xm1_saclac_interface::search::{Evaluation, ResultAddress};
use xm1_saclac_interface::stock::search::{evaluation, evaluation_address, EVALUATION_BYTES};

/// Completions 0x89 (live) and 0x9d (S1): the result at the address the completion names, as
/// `read` copies it out of calc's memory.
pub fn evaluated(payload: &[u8], read: impl FnOnce(ResultAddress) -> [u8; EVALUATION_BYTES]) -> Option<Evaluation> {
    evaluation_address(payload).map(|address| evaluation(&read(address)))
}
