use super::{Authenticator, Commitment, CryptoError, DhPrivate, DhPublic, EnrolleeMac, Iv, Nonce, PasswordHalf, SecretNonce, WpsKeys, pin_checksum};

fn scalar(value: u8) -> [u8; 192] {
    let mut bytes = [0; 192];
    bytes[191] = value;
    bytes
}
fn keys() -> WpsKeys {
    DhPrivate::from_bytes(scalar(2)).unwrap().derive_keys(
        &DhPublic::from_bytes(scalar(8)).unwrap(),
        &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16]),
    ).unwrap()
}
fn hex<const N: usize>(text: &str) -> [u8; N] {
    assert_eq!(text.len(), N * 2);
    let mut output = [0; N];
    for (index, byte) in output.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap();
    }
    output
}

#[test]
fn group5_small_exponents_and_shared_key_derivation() {
    let alice = DhPrivate::from_bytes(scalar(2)).unwrap();
    let bob = DhPrivate::from_bytes(scalar(3)).unwrap();
    assert_eq!(alice.public_key().as_bytes(), &scalar(4));
    assert_eq!(bob.public_key().as_bytes(), &scalar(8));
    let peer = alice.public_key();
    let bob_keys = bob.derive_keys(&peer, &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16])).unwrap();
    assert_eq!(bob_keys.authenticator(&[b"previous", b"current"]), keys().authenticator(&[b"previous", b"current"]));
}

#[test]
fn independently_generated_dh_kdf_hmac_vector() {
    let keys = keys();
    let expected = Authenticator(hex("4cb3943194611f6a"));
    assert_eq!(keys.authenticator(&[b"previous", b"current"]), expected);
    assert!(keys.verify_authenticator(&[b"previous", b"current"], &expected));
    for bit in 0..64 {
        let mut changed = expected;
        changed.0[bit / 8] ^= 1 << (bit % 8);
        assert!(!keys.verify_authenticator(&[b"previous", b"current"], &changed));
    }
    assert!(!keys.verify_authenticator(&[b"previous", b"changed"], &expected));
}

#[test]
fn independently_generated_aes_cbc_keywrap_vector() {
    let keys = keys();
    let plaintext = [0x10, 0x16, 0, 4, 1, 2, 3, 4];
    let expected: [u8; 48] = hex("03030303030303030303030303030303c06caeee7f7ae2dd5fbf9e2c091912f2995a02f1feeac0f3a29333896e79d123");
    let mut output = [0; 64];
    assert_eq!(keys.encrypt_settings(&Iv([3; 16]), &plaintext, &mut output), Ok(48));
    assert_eq!(&output[..48], expected);
    assert_eq!(keys.decrypt_settings(&expected, &mut output), Ok(8));
    assert_eq!(&output[..8], plaintext);
    assert!(output[8..32].iter().all(|byte| *byte == 0));
    for position in 0..expected.len() {
        let mut changed = expected;
        changed[position] ^= 1;
        output.fill(0xa5);
        assert_eq!(keys.decrypt_settings(&changed, &mut output), Err(CryptoError::InvalidEncryptedSettings));
        assert!(output[..32].iter().all(|byte| *byte == 0));
    }
}

#[test]
fn independently_generated_password_commitment_vector() {
    let keys = keys();
    let proof = keys.password_proofs(b"12345670").unwrap();
    let enrollee = DhPublic::from_bytes(scalar(4)).unwrap();
    let registrar = DhPublic::from_bytes(scalar(8)).unwrap();
    let secret = SecretNonce::from_bytes([4; 16]);
    let expected = Commitment(hex("af4f27807c31aa4d14bbf787eb9159a1cfac5809f18c93c85e289ec30c6de8cd"));
    assert_eq!(proof.commitment(&secret, &enrollee, &registrar, PasswordHalf::First), expected);
    assert!(proof.verify_commitment(&secret, &enrollee, &registrar, PasswordHalf::First, &expected));
    assert!(!proof.verify_commitment(&secret, &enrollee, &registrar, PasswordHalf::Second, &expected));
    assert!(!proof.verify_commitment(&secret, &registrar, &enrollee, PasswordHalf::First, &expected));
}

#[test]
fn rejects_invalid_keys_passwords_and_truncated_settings() {
    for value in [0, 1] {
        assert!(matches!(DhPublic::from_bytes(scalar(value)), Err(CryptoError::InvalidPublicKey)));
        assert!(matches!(DhPrivate::from_bytes(scalar(value)), Err(CryptoError::InvalidPrivateKey)));
    }
    assert!(matches!(DhPublic::from_bytes([0xff; 192]), Err(CryptoError::InvalidPublicKey)));
    assert!(matches!(DhPrivate::from_bytes([0xff; 192]), Err(CryptoError::InvalidPrivateKey)));
    let keys = keys();
    for password in [b"12345671".as_slice(), b"123", b"1234567a"] {
        assert!(matches!(keys.password_proofs(password), Err(CryptoError::InvalidPassword)));
    }
    assert!(keys.password_proofs(b"00000000").is_ok());
    assert!(keys.password_proofs(b"1234").is_ok());
    let mut output = [0; 64];
    for length in 0..32 {
        assert_eq!(keys.decrypt_settings(&[0; 64][..length], &mut output), Err(CryptoError::InvalidEncryptedSettings));
    }
    assert_eq!(keys.encrypt_settings(&Iv([3; 16]), b"settings", &mut [0; 47]), Err(CryptoError::OutputTooSmall));
    assert_eq!(keys.decrypt_settings(&[0; 48], &mut [0; 31]), Err(CryptoError::OutputTooSmall));
}

#[test]
fn pin_checksum_vectors() {
    assert_eq!(pin_checksum(1_234_567), 0);
    assert_eq!(pin_checksum(0), 0);
    assert_eq!(pin_checksum(1_111_111), 5);
}

#[test]
fn rejects_subgroup_order_exponent_and_checks_prime_boundaries() {
    let q = hex("7fffffffffffffffe487ed5110b4611a62633145c06e0e68948127044533e63a0105df531d89cd9128a5043cc71a026ef7ca8cd9e69d218d98158536f92f8a1ba7f09ab6b6a8e122f242dabb312f3f637a262174d31bf6b585ffae5b7a035bf6f71c35fdad44cfd2d74f9208be258ff324943328f6722d9ee1003e5c50b1df82cc6d241b0e2ae9cd348b1fd47e9267afc1b2ae91ee51d6cb0e3179ab1042a95dcf6a9483b84b4b36b3861aa7255e4c0278ba36046511b993ffffffffffffffff");
    assert!(matches!(DhPrivate::from_bytes(q), Err(CryptoError::InvalidPrivateKey)));
    let mut boundary: [u8; 192] = hex("ffffffffffffffffc90fdaa22168c234c4c6628b80dc1cd129024e088a67cc74020bbea63b139b22514a08798e3404ddef9519b3cd3a431b302b0a6df25f14374fe1356d6d51c245e485b576625e7ec6f44c42e9a637ed6b0bff5cb6f406b7edee386bfb5a899fa5ae9f24117c4b1fe649286651ece45b3dc2007cb8a163bf0598da48361c55d39a69163fa8fd24cf5f83655d23dca3ad961c62f356208552bb9ed529077096966d670c354e4abc9804f1746c08ca237327fffffffffffffffd");
    assert!(DhPrivate::from_bytes(boundary).is_ok());
    let non_subgroup_peer = DhPublic::from_bytes(boundary).unwrap();
    assert_eq!(DhPrivate::from_bytes(scalar(2)).unwrap().derive_keys(&non_subgroup_peer, &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16])).unwrap().authenticator(&[b"previous", b"current"]),
        DhPrivate::from_bytes(scalar(2)).unwrap().derive_keys(&DhPublic::from_bytes(scalar(2)).unwrap(), &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16])).unwrap().authenticator(&[b"previous", b"current"]));
    for last_byte in [0xfe, 0xff] {
        boundary[191] = last_byte;
        assert!(matches!(DhPrivate::from_bytes(boundary), Err(CryptoError::InvalidPrivateKey)));
        assert!(matches!(DhPublic::from_bytes(boundary), Err(CryptoError::InvalidPublicKey)));
    }
}

#[test]
fn independently_generated_full_width_group5_vector() {
    let alice = DhPrivate::from_bytes([0x42; 192]).unwrap();
    let bob = DhPrivate::from_bytes([0x69; 192]).unwrap();
    let public_a = DhPublic::from_bytes(hex("d7d676f1f05687fd3cdfbf474755cb2d2f6041e1ad8c67d991f9ffa2505ebd71a0bc3c67ad7fcda5d6f2a661370ff02ba64421d2fcd45d93e142e2b7425769750b465f4c3be9e10def53c5f2bf37dc979b5b71ac7d8ece415abc97e7442b9cdd93e23e40f2edae26cc8c2330049d6737065cf14ec01a93a06d12ae19b6eeb015cfd2156cd54bb6e384e0c8c5d3ab3de5758d1169f4d1a9a11a32c3ce0749deb25d19a0a19473882060a2049fea909ac67c8603ccf3d73067e524eac26debdd05")).unwrap();
    let public_b = DhPublic::from_bytes(hex("15433f467b0c119975805cb50f279673abdb80d8ec67cf759d2f5818dd42c18a066eec09ebde07792b2a2433e53fc3c303300a974ce309dc0c0da6f4797ec6ddf308cee23d25527392a434f7bf5d23a02ff238fb211b00069d2c3f1d028b24b77df87a22536ad7303b0d527bf507dc995726024bf94aa0a1f3cb8aeb71480ce807ddad86dc6e86df28d652c2376e5dcb394fd14c1922aee64cd95ffe508626559d7698a4412f3c8c89a29ed1be237c85c89a55677125996a19188d97e5e5d6f8")).unwrap();
    assert_eq!(alice.public_key(), public_a);
    assert_eq!(bob.public_key(), public_b);
    let expected = Authenticator(hex("f39568e432578374"));
    for derived in [
        alice.derive_keys(&public_b, &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16])).unwrap(),
        bob.derive_keys(&public_a, &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16])).unwrap(),
    ] {
        assert_eq!(derived.authenticator(&[b"previous", b"current"]), expected);
    }
}

#[test]
fn settings_cover_all_padding_lengths_and_exact_capacity() {
    let keys = keys();
    for length in 0..32 {
        let plaintext = [0x53; 32];
        let expected_length = 16 + ((length + 12) / 16 + 1) * 16;
        let mut encrypted = [0; 64];
        let written = keys.encrypt_settings(&Iv([3; 16]), &plaintext[..length], &mut encrypted[..expected_length]).unwrap();
        assert_eq!(written, expected_length);
        let mut decrypted = [0; 48];
        assert_eq!(keys.decrypt_settings(&encrypted[..written], &mut decrypted[..written - 16]), Ok(length));
        assert_eq!(&decrypted[..length], &plaintext[..length]);
    }
}

#[test]
fn rejects_independently_encrypted_bad_or_misplaced_kwa() {
    let keys = keys();
    for ciphertext in [
        "030303030303030303030303030303033b835897a31ca8eb108e39160aafabcd0a1ab93bdd31afa8e6a8de2b57b32b74",
        "030303030303030303030303030303032df822636838b8adedfe14ea6c0e40986a797050c35608fd7518444d3903513a",
        "030303030303030303030303030303039421e958ef257fc8fb60991dab6e3eeb7d32cd8438cd768857650f9ca7d006e1",
    ] {
        let mut plaintext = [0xa5; 32];
        assert_eq!(keys.decrypt_settings(&hex::<48>(ciphertext), &mut plaintext), Err(CryptoError::InvalidEncryptedSettings));
        assert_eq!(plaintext, [0; 32]);
    }
}

#[test]
fn second_half_commitment_vector_and_each_input_binding() {
    let keys = keys();
    let proofs = keys.password_proofs(b"12345670").unwrap();
    let enrollee = DhPublic::from_bytes(scalar(4)).unwrap();
    let registrar = DhPublic::from_bytes(scalar(8)).unwrap();
    let secret = SecretNonce::from_bytes([4; 16]);
    let expected = Commitment(hex("2d4945394969667735fb1efe5a63e26681532e8157c6f148f5613c3a32cc7abb"));
    assert_eq!(proofs.commitment(&secret, &enrollee, &registrar, PasswordHalf::Second), expected);
    assert!(!proofs.verify_commitment(&SecretNonce::from_bytes([5; 16]), &enrollee, &registrar, PasswordHalf::Second, &expected));
    assert!(!proofs.verify_commitment(&secret, &registrar, &registrar, PasswordHalf::Second, &expected));
    assert!(!proofs.verify_commitment(&secret, &enrollee, &enrollee, PasswordHalf::Second, &expected));
    assert!(!keys.password_proofs(b"1234").unwrap().verify_commitment(&secret, &enrollee, &registrar, PasswordHalf::Second, &expected));
    let changed_keys = DhPrivate::from_bytes(scalar(3)).unwrap().derive_keys(&registrar, &Nonce([1; 16]), &EnrolleeMac([2, 0, 0, 0, 0, 1]), &Nonce([2; 16])).unwrap();
    assert!(!changed_keys.password_proofs(b"12345670").unwrap().verify_commitment(&secret, &enrollee, &registrar, PasswordHalf::Second, &expected));
}
