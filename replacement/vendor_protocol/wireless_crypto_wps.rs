//! WPS group-5 and message cryptography for X-M1 1.50.
//! Source: x70view 55436fc85c0560dd252b0410af6f2427fe198cd3, hostap
//! src/wps/wps_common{,_rest}.c and src/crypto/dh_groups.c.

use xm1_wireless_crypto::{crypto_bigint, hmac, sha2, subtle, zeroize};
use crypto_bigint::{Encoding, U1536, modular::runtime_mod::{DynResidue, DynResidueParams}};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use subtle::{ConstantTimeEq, ConstantTimeLess};
use zeroize::{Zeroize, Zeroizing};
use super::Aes128;

const PRIME: U1536 = U1536::from_be_hex(concat!(
    "FFFFFFFFFFFFFFFFC90FDAA22168C234C4C6628B80DC1CD1",
    "29024E088A67CC74020BBEA63B139B22514A08798E3404DD",
    "EF9519B3CD3A431B302B0A6DF25F14374FE1356D6D51C245",
    "E485B576625E7EC6F44C42E9A637ED6B0BFF5CB6F406B7ED",
    "EE386BFB5A899FA5AE9F24117C4B1FE649286651ECE45B3D",
    "C2007CB8A163BF0598DA48361C55D39A69163FA8FD24CF5F",
    "83655D23DCA3AD961C62F356208552BB9ED529077096966D",
    "670C354E4ABC9804F1746C08CA237327FFFFFFFFFFFFFFFF"
));
const SUBGROUP_ORDER: U1536 = PRIME.wrapping_sub(&U1536::ONE).shr_vartime(1);
const PARAMS: DynResidueParams<{ U1536::LIMBS }> = DynResidueParams::new(&PRIME);
const KWA_HEADER: [u8; 4] = [0x10, 0x1e, 0, 8];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CryptoError { InvalidPrivateKey, InvalidPublicKey, InvalidPassword, OutputTooSmall, InvalidEncryptedSettings }
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Nonce(pub [u8; 16]);
pub struct SecretNonce(Zeroizing<[u8; 16]>);
impl SecretNonce {
    pub fn from_bytes(mut bytes: [u8; 16]) -> Self {
        let nonce = Self(Zeroizing::new(bytes));
        bytes.zeroize();
        nonce
    }
    pub fn as_bytes(&self) -> &[u8; 16] { &self.0 }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Authenticator(pub [u8; 8]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Commitment(pub [u8; 32]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EnrolleeMac(pub [u8; 6]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Iv(pub [u8; 16]);
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DhPublic([u8; 192]);
pub struct DhPrivate(Zeroizing<U1536>);

impl DhPublic {
    pub fn from_bytes(bytes: [u8; 192]) -> Result<Self, CryptoError> {
        let value = U1536::from_be_bytes(bytes);
        let upper = PRIME.wrapping_sub(&U1536::ONE);
        if bool::from(U1536::ONE.ct_lt(&value) & value.ct_lt(&upper)) {
            Ok(Self(bytes))
        } else { Err(CryptoError::InvalidPublicKey) }
    }
    pub fn as_bytes(&self) -> &[u8; 192] { &self.0 }
}
impl DhPrivate {
    /// Input is a fresh uniformly random 192-byte candidate; retry rejected candidates.
    pub fn from_bytes(mut bytes: [u8; 192]) -> Result<Self, CryptoError> {
        let value = Zeroizing::new(U1536::from_be_bytes(bytes));
        bytes.zeroize();
        if !bool::from(U1536::ONE.ct_lt(&*value) & value.ct_lt(&PRIME.wrapping_sub(&U1536::ONE)) & !value.ct_eq(&SUBGROUP_ORDER)) {
            return Err(CryptoError::InvalidPrivateKey);
        }
        Ok(Self(value))
    }
    pub fn public_key(&self) -> DhPublic {
        let base = DynResidue::new(&U1536::from_u8(2), PARAMS);
        DhPublic(base.pow(&self.0).retrieve().to_be_bytes())
    }
    pub fn derive_keys(self, peer: &DhPublic, enrollee_nonce: &Nonce, mac: &EnrolleeMac, registrar_nonce: &Nonce) -> Result<WpsKeys, CryptoError> {
        let mut shared = DynResidue::new(&U1536::from_be_bytes(peer.0), PARAMS).pow(&self.0);
        let integer = Zeroizing::new(shared.retrieve());
        shared.as_montgomery_mut().zeroize();
        let bytes = Zeroizing::new(integer.to_be_bytes());
        if bool::from(integer.ct_eq(&U1536::ONE) | integer.ct_eq(&U1536::ZERO)) {
            return Err(CryptoError::InvalidPublicKey);
        }
        let dhkey = Zeroizing::new(<[u8; 32]>::from(Sha256::digest(&*bytes)));
        let kdk = Zeroizing::new(hmac(&dhkey[..], &[&enrollee_nonce.0, &mac.0, &registrar_nonce.0]));
        let mut expanded = Zeroizing::new([0u8; 80]);
        for (index, chunk) in expanded.chunks_mut(32).enumerate() {
            let digest = Zeroizing::new(hmac(&kdk[..], &[&((index + 1) as u32).to_be_bytes(), b"Wi-Fi Easy and Secure Key Derivation", &640u32.to_be_bytes()]));
            chunk.copy_from_slice(&digest[..chunk.len()]);
        }
        let mut keys = WpsKeys { auth: [0; 32], wrap: [0; 16], emsk: [0; 32] };
        keys.auth.copy_from_slice(&expanded[..32]);
        keys.wrap.copy_from_slice(&expanded[32..48]);
        keys.emsk.copy_from_slice(&expanded[48..]);
        Ok(keys)
    }
}

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut state = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts every key length");
    for part in parts { state.update(part); }
    state.finalize().into_bytes().into()
}

pub struct WpsKeys { auth: [u8; 32], wrap: [u8; 16], emsk: [u8; 32] }
impl Drop for WpsKeys {
    fn drop(&mut self) { self.auth.zeroize(); self.wrap.zeroize(); self.emsk.zeroize(); }
}
impl WpsKeys {
    pub fn authenticator(&self, parts: &[&[u8]]) -> Authenticator {
        let digest = Zeroizing::new(hmac(&self.auth, parts));
        let mut result = [0; 8];
        result.copy_from_slice(&digest[..8]);
        Authenticator(result)
    }
    pub fn verify_authenticator(&self, parts: &[&[u8]], expected: &Authenticator) -> bool {
        bool::from(self.authenticator(parts).0.ct_eq(&expected.0))
    }
    pub fn password_proofs(&self, password: &[u8]) -> Result<PasswordProofs<'_>, CryptoError> {
        if !matches!(password.len(), 4 | 8) || !password.iter().all(u8::is_ascii_digit) {
            return Err(CryptoError::InvalidPassword);
        }
        if password.len() == 8 && !valid_pin(password) { return Err(CryptoError::InvalidPassword); }
        let split = password.len().div_ceil(2);
        let first = Zeroizing::new(hmac(&self.auth, &[&password[..split]]));
        let second = Zeroizing::new(hmac(&self.auth, &[&password[split..]]));
        let mut proofs = PasswordProofs { keys: self, halves: [[0; 16]; 2] };
        proofs.halves[0].copy_from_slice(&first[..16]);
        proofs.halves[1].copy_from_slice(&second[..16]);
        Ok(proofs)
    }
    /// The caller supplies a fresh unpredictable IV for each encryption.
    pub fn encrypt_settings(&self, iv: &Iv, plaintext: &[u8], output: &mut [u8]) -> Result<usize, CryptoError> {
        let unpadded = plaintext.len().checked_add(12).ok_or(CryptoError::OutputTooSmall)?;
        let pad = 16 - unpadded % 16;
        let total = unpadded.checked_add(pad + 16).ok_or(CryptoError::OutputTooSmall)?;
        if output.len() < total { return Err(CryptoError::OutputTooSmall); }
        output[..16].copy_from_slice(&iv.0);
        output[16..16 + plaintext.len()].copy_from_slice(plaintext);
        let kwa = self.authenticator(&[plaintext]);
        output[16 + plaintext.len()..20 + plaintext.len()].copy_from_slice(&KWA_HEADER);
        output[20 + plaintext.len()..16 + unpadded].copy_from_slice(&kwa.0);
        output[16 + unpadded..total].fill(pad as u8);
        encrypt_cbc(&self.wrap, iv, &mut output[16..total]);
        Ok(total)
    }
    /// Verify the enclosing message Authenticator before calling this method.
    pub fn decrypt_settings(&self, encrypted: &[u8], output: &mut [u8]) -> Result<usize, CryptoError> {
        if encrypted.len() < 32 || encrypted.len() % 16 != 0 { return Err(CryptoError::InvalidEncryptedSettings); }
        let length = encrypted.len() - 16;
        if output.len() < length { return Err(CryptoError::OutputTooSmall); }
        output[..length].copy_from_slice(&encrypted[16..]);
        let mut iv = [0; 16];
        iv.copy_from_slice(&encrypted[..16]);
        decrypt_cbc(&self.wrap, &Iv(iv), &mut output[..length]);
        let pad = output[length - 1] as usize;
        let mut invalid = (pad == 0 || pad > 16) as u8;
        for offset in 0..16 {
            let mask = 0u8.wrapping_sub((offset < pad) as u8);
            invalid |= mask & (output[length - 1 - offset] ^ pad as u8);
        }
        let unpadded = length.saturating_sub(pad);
        if invalid != 0 || unpadded < 12 {
            output[..length].zeroize();
            return Err(CryptoError::InvalidEncryptedSettings);
        }
        let end = unpadded - 12;
        let mut expected = [0; 8];
        expected.copy_from_slice(&output[end + 4..unpadded]);
        let valid = bool::from(output[end..end + 4].ct_eq(&KWA_HEADER)) & self.verify_authenticator(&[&output[..end]], &Authenticator(expected));
        if !valid {
            output[..length].zeroize();
            return Err(CryptoError::InvalidEncryptedSettings);
        }
        output[end..length].zeroize();
        Ok(end)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PasswordHalf { First, Second }
pub struct PasswordProofs<'a> { keys: &'a WpsKeys, halves: [[u8; 16]; 2] }
impl Drop for PasswordProofs<'_> { fn drop(&mut self) { self.halves.zeroize(); } }
impl PasswordProofs<'_> {
    pub fn commitment(&self, secret: &SecretNonce, enrollee: &DhPublic, registrar: &DhPublic, half: PasswordHalf) -> Commitment {
        let index = match half { PasswordHalf::First => 0, PasswordHalf::Second => 1 };
        Commitment(hmac(&self.keys.auth, &[secret.as_bytes(), &self.halves[index], &enrollee.0, &registrar.0]))
    }
    pub fn verify_commitment(&self, secret: &SecretNonce, enrollee: &DhPublic, registrar: &DhPublic, half: PasswordHalf, expected: &Commitment) -> bool {
        bool::from(self.commitment(secret, enrollee, registrar, half).0.ct_eq(&expected.0))
    }
}

pub const fn pin_checksum(mut seven_digits: u32) -> u8 {
    let mut sum = 0;
    while seven_digits != 0 {
        sum += 3 * (seven_digits % 10);
        seven_digits /= 10;
        sum += seven_digits % 10;
        seven_digits /= 10;
    }
    ((10 - sum % 10) % 10) as u8
}
fn valid_pin(pin: &[u8]) -> bool {
    let mut number = 0;
    for digit in &pin[..7] { number = number * 10 + (digit - b'0') as u32; }
    bool::from(pin_checksum(number).ct_eq(&(pin[7] - b'0')))
}
fn encrypt_cbc(key: &[u8; 16], iv: &Iv, data: &mut [u8]) {
    let aes = Aes128::new(key);
    let mut chain = iv.0;
    for bytes in data.chunks_exact_mut(16) {
        let mut block = Zeroizing::new([0; 16]);
        for index in 0..16 { block[index] = bytes[index] ^ chain[index]; }
        aes.encrypt(&mut block);
        chain.copy_from_slice(&block[..]);
        bytes.copy_from_slice(&chain);
    }
    chain.zeroize();
}
fn decrypt_cbc(key: &[u8; 16], iv: &Iv, data: &mut [u8]) {
    let aes = Aes128::new(key);
    let mut chain = iv.0;
    for bytes in data.chunks_exact_mut(16) {
        let mut encrypted = [0; 16];
        encrypted.copy_from_slice(bytes);
        let mut block = Zeroizing::new(encrypted);
        aes.decrypt(&mut block);
        for index in 0..16 { block[index] ^= chain[index]; }
        bytes.copy_from_slice(&block[..]);
        chain = encrypted;
    }
    chain.zeroize();
}

#[cfg(test)]
#[path = "wireless_crypto_wps_tests.rs"]
mod tests;
