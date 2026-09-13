use std::sync::Arc;

use sarmg_secret::{SecretBytes, SecretKey};
use sarmg_secret_envelope::EnvelopeDomain;

struct ClientAuthorizationEnvelope;

impl EnvelopeDomain for ClientAuthorizationEnvelope {
    const DOMAIN: &'static [u8] = b"host-monitoring/client-instance-authorization";
    const REVISION: u16 = 1;
}

#[derive(Clone)]
pub struct SecretBox(Arc<SecretKey<32>>);

impl SecretBox {
    pub fn new(key: [u8; 32]) -> Self {
        Self(Arc::new(SecretKey::new(key)))
    }

    pub fn encrypt(&self, instance_id: uuid::Uuid, value: &str) -> anyhow::Result<Vec<u8>> {
        Ok(sarmg_secret_envelope::seal::<ClientAuthorizationEnvelope>(
            &self.0,
            instance_id.as_bytes(),
            &SecretBytes::new(value.as_bytes().to_vec()),
        )?)
    }

    pub fn decrypt(&self, instance_id: uuid::Uuid, value: &[u8]) -> anyhow::Result<String> {
        anyhow::ensure!(
            (64..=1024).contains(&value.len()),
            "client authorization envelope has an invalid size"
        );
        let plaintext = sarmg_secret_envelope::open::<ClientAuthorizationEnvelope>(
            &self.0,
            instance_id.as_bytes(),
            value,
        )?;
        Ok(String::from_utf8(plaintext.expose().to_vec())?)
    }
}
