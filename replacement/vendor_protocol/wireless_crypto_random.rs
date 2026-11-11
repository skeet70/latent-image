//! X-M1 tick-byte generator and hostap entropy-pool mixing.
//! This source-bound generator does not establish cryptographic entropy strength.

use super::hmac_sha1;
use xm1_wireless_crypto::zeroize::Zeroize;
use xm1_rtos_own_first::{self as rtos_own_first, Domain, DomainWords, GuestAddress, Machine, ServiceStatus, SystemTime};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RandomError { ScratchAlignment, Clock(ServiceStatus) }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockScratch(GuestAddress);
impl ClockScratch {
    pub fn new(address: GuestAddress) -> Result<Self, RandomError> {
        if address.0 == 0 || address.0 & 3 != 0 { Err(RandomError::ScratchAlignment) } else { Ok(Self(address)) }
    }
}
pub struct RtosClock<'a, M: Machine> { machine: &'a M, words: DomainWords, scratch: ClockScratch }
impl<'a, M: Machine> RtosClock<'a, M> {
    pub fn new(machine: &'a M, domain: Domain, scratch: ClockScratch) -> Self { Self { machine, words: domain.words(), scratch } }
    pub fn sample(&self) -> Result<SystemTime, RandomError> {
        let status = rtos_own_first::slot_100(self.machine, &self.words, self.scratch.0);
        if status != ServiceStatus::OK { return Err(RandomError::Clock(status)); }
        Ok(SystemTime(self.machine.load_word(self.scratch.0)))
    }
}

pub struct RandomPool { pool: [u32; 32], input_rotate: u32, position: usize, entropy: u32, collected: u32, count: u32 }
impl Default for RandomPool { fn default() -> Self { Self::new() } }
impl RandomPool {
    pub const fn new() -> Self { Self { pool: [0; 32], input_rotate: 0, position: 0, entropy: 0, collected: 0, count: 0 } }
    pub fn add_randomness<M: Machine>(&mut self, clock: &RtosClock<'_, M>, input: &[u8]) -> Result<(), RandomError> {
        self.count = self.count.wrapping_add(1);
        if self.entropy > 1000 && self.count & 0x3ff != 0 { return Ok(()); }
        let time = clock.sample()?.0;
        self.mix(&(time / 1000).to_le_bytes());
        self.mix(&((time % 1000) * 1000).to_le_bytes());
        self.mix(input);
        self.entropy = self.entropy.wrapping_add(1);
        self.collected = self.collected.wrapping_add(1);
        Ok(())
    }
    pub fn generate<M: Machine>(&mut self, clock: &RtosClock<'_, M>, output: &mut [u8]) -> Result<(), RandomError> {
        for index in 0..output.len() {
            match clock.sample() {
                Ok(time) => output[index] = time.0 as u8,
                Err(error) => { output.zeroize(); return Err(error); }
            }
        }
        for chunk in output.chunks_mut(16) {
            let mut extracted = self.extract();
            for (byte, pool_byte) in chunk.iter_mut().zip(extracted) { *byte ^= pool_byte; }
            extracted.zeroize();
        }
        self.entropy = self.entropy.saturating_sub(output.len() as u32);
        Ok(())
    }
    fn mix(&mut self, input: &[u8]) {
        const TWIST: [u32; 8] = [0x00000000,0x3b6e20c8,0x76dc4190,0x4db26158,0xedb88320,0xd6d6a3e8,0x9b64c2b0,0xa00ae278];
        for byte in input {
            let mut word = (*byte as u32).rotate_left(self.input_rotate & 31);
            self.input_rotate = self.input_rotate.wrapping_add(if self.position == 0 { 14 } else { 7 });
            self.position = self.position.wrapping_sub(1) & 31;
            for tap in [0,26,20,14,7,1] { word ^= self.pool[(self.position + tap) & 31]; }
            let twist = TWIST.iter().enumerate().fold(0, |selected, (index, value)| {
                let mask = (((word & 7) ^ index as u32).wrapping_sub(1) >> 31).wrapping_neg();
                selected | (*value & mask)
            });
            self.pool[self.position] = (word >> 3) ^ twist;
        }
    }
    fn extract(&mut self) -> [u8; 16] {
        let mut snapshot = [0; 128];
        for (bytes, word) in snapshot.chunks_exact_mut(4).zip(self.pool) { bytes.copy_from_slice(&word.to_le_bytes()); }
        let mut digest = hmac_sha1(&[0; 20], &[&snapshot]);
        self.mix(&digest);
        for index in 0..16 { snapshot[index * 4..index * 4 + 4].copy_from_slice(&self.pool[self.position.wrapping_sub(index) & 31].to_le_bytes()); }
        digest.zeroize();
        digest = hmac_sha1(&[0; 20], &[&snapshot[..64]]);
        for index in 0..4 { digest[index] ^= digest[index + 16]; }
        let mut output = [0; 16]; output.copy_from_slice(&digest[..16]);
        snapshot.zeroize(); digest.zeroize();
        output
    }
}
impl Drop for RandomPool { fn drop(&mut self) { self.pool.zeroize(); self.input_rotate.zeroize(); self.position.zeroize(); self.entropy.zeroize(); self.collected.zeroize(); self.count.zeroize(); } }

#[cfg(test)]
#[path = "wireless_crypto_random_tests.rs"]
mod tests;
