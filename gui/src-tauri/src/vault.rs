use std::collections::BTreeMap;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    ChaCha20Poly1305,
    aead::{Aead, AeadCore, KeyInit, OsRng, rand_core::RngCore},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::secrets::SecretError;

const V1_MEMORY_KIB: u32 = 19 * 1024;
const V1_ITERATIONS: u32 = 2;
const V1_PARALLELISM: u32 = 1;

#[derive(Deserialize, Serialize)]
pub(crate) struct EncryptedVault {
    version: u8,
    salt: [u8; 16],
    nonce: [u8; 12],
    pub(crate) ciphertext: Vec<u8>,
}

impl EncryptedVault {
    pub(crate) fn seal(
        entries: &BTreeMap<String, Vec<u8>>,
        passphrase: &str,
    ) -> Result<Self, SecretError> {
        let mut salt = [0_u8; 16];
        OsRng.fill_bytes(&mut salt);
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
        let key = derive_key(passphrase, &salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| SecretError::VaultAuthentication)?;
        let plaintext = Zeroizing::new(serde_json::to_vec(entries)?);
        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_ref())
            .map_err(|_| SecretError::VaultAuthentication)?;

        Ok(Self {
            version: 1,
            salt,
            nonce: nonce.into(),
            ciphertext,
        })
    }

    pub(crate) fn open(&self, passphrase: &str) -> Result<BTreeMap<String, Vec<u8>>, SecretError> {
        if self.version != 1 {
            return Err(SecretError::UnsupportedVaultVersion);
        }
        let key = derive_key(passphrase, &self.salt)?;
        let cipher = ChaCha20Poly1305::new_from_slice(key.as_ref())
            .map_err(|_| SecretError::VaultAuthentication)?;
        let plaintext = Zeroizing::new(
            cipher
                .decrypt((&self.nonce).into(), self.ciphertext.as_ref())
                .map_err(|_| SecretError::VaultAuthentication)?,
        );
        Ok(serde_json::from_slice(&plaintext)?)
    }
}

fn derive_key(passphrase: &str, salt: &[u8; 16]) -> Result<Zeroizing<[u8; 32]>, SecretError> {
    let mut key = Zeroizing::new([0_u8; 32]);
    let params = Params::new(
        V1_MEMORY_KIB,
        V1_ITERATIONS,
        V1_PARALLELISM,
        Some(key.len()),
    )
    .map_err(|_| SecretError::KeyDerivation)?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, key.as_mut())
        .map_err(|_| SecretError::KeyDerivation)?;
    Ok(key)
}
