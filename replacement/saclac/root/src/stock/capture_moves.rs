#![forbid(unsafe_code)]

use xm1_saclac_interface::stock::StockCallbackWord;

/// X-M1 76e6cc..76e704: callback, full target, then the four caller words,24 bytes.
pub(crate) fn absolute(callback: StockCallbackWord, target: u32, words: [u32; 4]) -> [u8; 24] {
    let mut bytes = [0; 24];
    for (out, value) in bytes.chunks_exact_mut(4).zip([callback.0, target, words[0], words[1], words[2], words[3]]) {
        out.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}
