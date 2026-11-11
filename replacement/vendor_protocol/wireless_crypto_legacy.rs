//! MD5 and RC4 for WPA descriptor version 1.
//! Source scope: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, hostap crypto.

#[derive(Clone)]
struct Md5 { state: [u32; 4], block: [u8; 64], used: usize, length: u64 }
impl Md5 {
    fn new() -> Self { Self { state: [0x67452301,0xefcdab89,0x98badcfe,0x10325476], block: [0;64], used: 0, length: 0 } }
    fn update(&mut self, mut input: &[u8]) {
        self.length = self.length.wrapping_add(input.len() as u64);
        while !input.is_empty() {
            let n = (64-self.used).min(input.len());
            self.block[self.used..self.used+n].copy_from_slice(&input[..n]);
            self.used += n; input = &input[n..];
            if self.used == 64 { self.compress(); self.used = 0; }
        }
    }
    fn compress(&mut self) {
        const K: [u32;64] = [
            0xd76aa478,0xe8c7b756,0x242070db,0xc1bdceee,0xf57c0faf,0x4787c62a,0xa8304613,0xfd469501,
            0x698098d8,0x8b44f7af,0xffff5bb1,0x895cd7be,0x6b901122,0xfd987193,0xa679438e,0x49b40821,
            0xf61e2562,0xc040b340,0x265e5a51,0xe9b6c7aa,0xd62f105d,0x02441453,0xd8a1e681,0xe7d3fbc8,
            0x21e1cde6,0xc33707d6,0xf4d50d87,0x455a14ed,0xa9e3e905,0xfcefa3f8,0x676f02d9,0x8d2a4c8a,
            0xfffa3942,0x8771f681,0x6d9d6122,0xfde5380c,0xa4beea44,0x4bdecfa9,0xf6bb4b60,0xbebfbc70,
            0x289b7ec6,0xeaa127fa,0xd4ef3085,0x04881d05,0xd9d4d039,0xe6db99e5,0x1fa27cf8,0xc4ac5665,
            0xf4292244,0x432aff97,0xab9423a7,0xfc93a039,0x655b59c3,0x8f0ccc92,0xffeff47d,0x85845dd1,
            0x6fa87e4f,0xfe2ce6e0,0xa3014314,0x4e0811a1,0xf7537e82,0xbd3af235,0x2ad7d2bb,0xeb86d391];
        const S: [[u32;4];4] = [[7,12,17,22],[5,9,14,20],[4,11,16,23],[6,10,15,21]];
        let mut words = [0u32;16];
        for (word, bytes) in words.iter_mut().zip(self.block.chunks_exact(4)) { *word = u32::from_le_bytes(bytes.try_into().expect("word extent")); }
        let [mut a,mut b,mut c,mut d] = self.state;
        for i in 0..64 {
            let (f,g) = match i { 0..=15 => ((b&c)|(!b&d),i), 16..=31 => ((d&b)|(!d&c),(5*i+1)%16), 32..=47 => (b^c^d,(3*i+5)%16), _ => (c^(b|!d),(7*i)%16) };
            let next = b.wrapping_add(a.wrapping_add(f).wrapping_add(K[i]).wrapping_add(words[g]).rotate_left(S[i/16][i%4]));
            a=d; d=c; c=b; b=next;
        }
        for (state,value) in self.state.iter_mut().zip([a,b,c,d]) { *state = state.wrapping_add(value); }
    }
    fn finish(mut self) -> [u8;16] {
        let bits = self.length.wrapping_mul(8).to_le_bytes(); self.update(&[0x80]);
        let padding = if self.used <= 56 {56-self.used} else {120-self.used};
        self.update(&[0;64][..padding]); self.update(&bits);
        let mut output = [0;16]; for (out,word) in output.chunks_exact_mut(4).zip(self.state) {out.copy_from_slice(&word.to_le_bytes());} output
    }
}
pub fn md5(fragments: &[&[u8]]) -> [u8;16] { let mut hash=Md5::new(); for input in fragments {hash.update(input);} hash.finish() }
pub fn hmac_md5(key: &[u8], fragments: &[&[u8]]) -> [u8;16] {
    let mut pad=[0;64]; if key.len()>64 {pad[..16].copy_from_slice(&md5(&[key]));} else {pad[..key.len()].copy_from_slice(key);}
    for b in &mut pad {*b ^= 0x36;} let mut inner=Md5::new(); inner.update(&pad); for input in fragments {inner.update(input);}
    for b in &mut pad {*b ^= 0x36^0x5c;} let mut outer=Md5::new(); outer.update(&pad); outer.update(&inner.finish()); pad.fill(0); outer.finish()
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LegacyError { EmptyKey }
pub fn rc4_skip(key: &[u8], skip: usize, data: &mut [u8]) -> Result<(),LegacyError> {
    if key.is_empty() {return Err(LegacyError::EmptyKey);}
    let mut state=[0u8;256]; for (i,b) in state.iter_mut().enumerate() {*b=i as u8;}
    let mut j=0u8; for i in 0..256 {j=j.wrapping_add(state[i]).wrapping_add(key[i%key.len()]); state.swap(i,j as usize);}
    let (mut i,mut j)=(0u8,0u8);
    for _ in 0..skip {
        i=i.wrapping_add(1); j=j.wrapping_add(state[i as usize]); state.swap(i as usize,j as usize);
    }
    for output in data {
        i=i.wrapping_add(1); j=j.wrapping_add(state[i as usize]); state.swap(i as usize,j as usize);
        *output ^= state[state[i as usize].wrapping_add(state[j as usize]) as usize];
    }
    state.fill(0); Ok(())
}
#[cfg(test)]
#[path="wireless_crypto_legacy_tests.rs"]
mod tests;
