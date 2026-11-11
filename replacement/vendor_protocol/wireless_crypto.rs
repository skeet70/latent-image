//! Station key derivation and RFC 3394 key unwrap.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, hostap crypto.

#[path = "wireless_crypto_wps.rs"]
pub mod wps;
#[path = "wireless_crypto_legacy.rs"]
pub mod legacy;
#[path = "wireless_crypto_random.rs"]
pub mod random;

use core::fmt;
use xm1_wireless_crypto::{aes, zeroize::Zeroize};

#[derive(Clone, Eq, PartialEq)]
pub struct MasterKey([u8; 32]);
impl MasterKey {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
}
impl fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("MasterKey([redacted])") }
}
impl Drop for MasterKey { fn drop(&mut self) { self.0.zeroize(); } }

#[derive(Clone)]
pub struct PairwiseKeys { bytes: [u8; 64] }
impl PairwiseKeys {
    pub fn confirmation(&self) -> &[u8] { &self.bytes[..16] }
    pub fn encryption(&self) -> &[u8; 16] { self.bytes[16..32].try_into().expect("fixed key extent") }
    pub fn temporal(&self) -> &[u8; 16] { self.bytes[32..48].try_into().expect("fixed key extent") }
    pub fn temporal_tkip(&self) -> &[u8; 32] { self.bytes[32..64].try_into().expect("fixed key extent") }
}
impl fmt::Debug for PairwiseKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("PairwiseKeys([redacted])") }
}
impl Drop for PairwiseKeys { fn drop(&mut self) { self.bytes.zeroize(); } }

#[derive(Clone)]
struct Sha1 { state: [u32; 5], block: [u8; 64], used: usize, length: u64 }
impl Drop for Sha1 { fn drop(&mut self) { self.state.zeroize(); self.block.zeroize(); } }
impl Sha1 {
    fn new() -> Self { Self { state: [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0], block: [0; 64], used: 0, length: 0 } }
    fn update(&mut self, mut input: &[u8]) {
        self.length = self.length.wrapping_add(input.len() as u64);
        while !input.is_empty() {
            let count = (64 - self.used).min(input.len());
            self.block[self.used..self.used + count].copy_from_slice(&input[..count]);
            self.used += count;
            input = &input[count..];
            if self.used == 64 { self.compress(); self.used = 0; }
        }
    }
    fn compress(&mut self) {
        let mut words = [0u32; 80];
        for (word, bytes) in words[..16].iter_mut().zip(self.block.chunks_exact(4)) {
            *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        }
        for i in 16..80 { words[i] = (words[i - 3] ^ words[i - 8] ^ words[i - 14] ^ words[i - 16]).rotate_left(1); }
        let [mut a, mut b, mut c, mut d, mut e] = self.state;
        for (i, word) in words.into_iter().enumerate() {
            let (function, constant) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5a827999u32),
                20..=39 => (b ^ c ^ d, 0x6ed9eba1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8f1bbcdc),
                _ => (b ^ c ^ d, 0xca62c1d6),
            };
            let next = a.rotate_left(5).wrapping_add(function).wrapping_add(e).wrapping_add(constant).wrapping_add(word);
            e = d; d = c; c = b.rotate_left(30); b = a; a = next;
        }
        for (state, value) in self.state.iter_mut().zip([a, b, c, d, e]) { *state = state.wrapping_add(value); }
    }
    fn finish(mut self) -> [u8; 20] {
        let bits = self.length.wrapping_mul(8).to_be_bytes();
        self.update(&[0x80]);
        let zeroes = [0; 64];
        let padding = if self.used <= 56 { 56 - self.used } else { 120 - self.used };
        self.update(&zeroes[..padding]);
        self.update(&bits);
        let mut output = [0; 20];
        for (bytes, word) in output.chunks_exact_mut(4).zip(self.state) { bytes.copy_from_slice(&word.to_be_bytes()); }
        output
    }
}

struct HmacSha1 { inner: Sha1, outer: Sha1 }
impl HmacSha1 {
    fn new(key: &[u8]) -> Self {
        let mut pad = [0; 64];
        if key.len() > 64 { let mut hash = Sha1::new(); hash.update(key); pad[..20].copy_from_slice(&hash.finish()); }
        else { pad[..key.len()].copy_from_slice(key); }
        for byte in &mut pad { *byte ^= 0x36; }
        let mut inner = Sha1::new(); inner.update(&pad);
        for byte in &mut pad { *byte ^= 0x36 ^ 0x5c; }
        let mut outer = Sha1::new(); outer.update(&pad);
        Self { inner, outer }
    }
    fn authenticate(&self, fragments: &[&[u8]]) -> [u8; 20] {
        let mut inner = self.inner.clone();
        for fragment in fragments { inner.update(fragment); }
        let mut outer = self.outer.clone(); outer.update(&inner.finish()); outer.finish()
    }
}

pub fn hmac_sha1(key: &[u8], fragments: &[&[u8]]) -> [u8; 20] { HmacSha1::new(key).authenticate(fragments) }

pub fn derive_master_key(passphrase: &[u8], ssid: &[u8]) -> MasterKey {
    let hmac = HmacSha1::new(passphrase);
    let mut key = [0; 32];
    for (index, output) in key.chunks_mut(20).enumerate() {
        let counter = (index as u32 + 1).to_be_bytes();
        let mut u = hmac.authenticate(&[ssid, &counter]);
        let mut block = u;
        for _ in 1..4096 { u = hmac.authenticate(&[&u]); for (out, byte) in block.iter_mut().zip(u) { *out ^= byte; } }
        output.copy_from_slice(&block[..output.len()]);
    }
    MasterKey(key)
}

pub fn derive_pairwise_keys(master: &MasterKey, local: &[u8; 6], peer: &[u8; 6], local_nonce: &[u8; 32], peer_nonce: &[u8; 32]) -> PairwiseKeys {
    let (first_address, second_address) = if local < peer { (local, peer) } else { (peer, local) };
    let (first_nonce, second_nonce) = if local_nonce < peer_nonce { (local_nonce, peer_nonce) } else { (peer_nonce, local_nonce) };
    let hmac = HmacSha1::new(master.as_bytes());
    let mut bytes = [0; 64];
    for (index, chunk) in bytes.chunks_mut(20).enumerate() {
        let hash = hmac.authenticate(&[b"Pairwise key expansion\0", first_address, second_address, first_nonce, second_nonce, &[index as u8]]);
        chunk.copy_from_slice(&hash[..chunk.len()]);
    }
    PairwiseKeys { bytes }
}

pub fn equal_authenticator(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() { return false; }
    left.iter().zip(right).fold(0u8, |difference, (a, b)| difference | (a ^ b)) == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyWrapError { Length, Integrity }

pub(super) struct Aes128(aes::Aes128);
impl Aes128 {
    pub(super) fn new(key: &[u8; 16]) -> Self {
        use aes::cipher::KeyInit;
        Self(aes::Aes128::new(key.into()))
    }
    pub(super) fn encrypt(&self, block: &mut [u8; 16]) {
        use aes::cipher::BlockEncrypt;
        self.0.encrypt_block(block.into());
    }
    pub(super) fn decrypt(&self, block: &mut [u8; 16]) {
        use aes::cipher::BlockDecrypt;
        self.0.decrypt_block(block.into());
    }
}

pub fn unwrap_key_data(key: &[u8; 16], wrapped: &[u8], output: &mut [u8]) -> Result<usize, KeyWrapError> {
    if wrapped.len() < 24 || wrapped.len() % 8 != 0 || output.len() < wrapped.len() - 8 { return Err(KeyWrapError::Length); }
    let count = wrapped.len() / 8 - 1;
    let length = count * 8;
    output[..length].copy_from_slice(&wrapped[8..]);
    let mut a = [0; 8]; a.copy_from_slice(&wrapped[..8]);
    let aes = Aes128::new(key);
    for round in (0..6).rev() {
        for index in (1..=count).rev() {
            let counter = (count * round + index) as u64;
            let mut block = [0; 16];
            block[..8].copy_from_slice(&(u64::from_be_bytes(a) ^ counter).to_be_bytes());
            block[8..].copy_from_slice(&output[(index - 1) * 8..index * 8]);
            aes.decrypt(&mut block);
            a.copy_from_slice(&block[..8]);
            output[(index - 1) * 8..index * 8].copy_from_slice(&block[8..]);
        }
    }
    if !equal_authenticator(&a, &[0xa6; 8]) { output[..length].zeroize(); return Err(KeyWrapError::Integrity); }
    Ok(length)
}

pub fn wrap_key_data(key: &[u8; 16], plaintext: &[u8], output: &mut [u8]) -> Result<usize, KeyWrapError> {
    if plaintext.len() < 16 || plaintext.len() % 8 != 0 || output.len() < plaintext.len() + 8 { return Err(KeyWrapError::Length); }
    let count = plaintext.len() / 8;
    output[8..8 + plaintext.len()].copy_from_slice(plaintext);
    let mut a = [0xa6; 8];
    let aes = Aes128::new(key);
    for round in 0..6 {
        for index in 1..=count {
            let mut block = [0; 16];
            block[..8].copy_from_slice(&a);
            block[8..].copy_from_slice(&output[index * 8..(index + 1) * 8]);
            aes.encrypt(&mut block);
            let counter = (count * round + index) as u64;
            let mut upper = [0; 8]; upper.copy_from_slice(&block[..8]);
            a = (u64::from_be_bytes(upper) ^ counter).to_be_bytes();
            output[index * 8..(index + 1) * 8].copy_from_slice(&block[8..]);
        }
    }
    output[..8].copy_from_slice(&a);
    Ok(plaintext.len() + 8)
}

#[cfg(test)]
#[path = "wireless_crypto_tests.rs"]
mod tests;
