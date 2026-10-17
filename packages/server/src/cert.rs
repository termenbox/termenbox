//! Device certificates (DECISION-0062, DECISION-0116). The hosted function signs (provider, account id, device
//! public key, expiry, `kid`) with an Ed25519 key after a GitHub login; the browser proves it holds the device key
//! by signing a per-connection challenge that names this server. The server never sees the GitHub token.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use ed25519_dalek::{Signature as EdSignature, Verifier as _, VerifyingKey as EdKey};
use p256::ecdsa::{Signature as P256Signature, VerifyingKey as P256Key};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// A certificate may not outlive this, whatever the signer wrote (the function issues 7 days).
pub const MAX_LIFETIME_SECS: u64 = 8 * 24 * 3600;
/// Public keys of the hosted function, as `kid=<base64url Ed25519 key>` lines.
const DEFAULT_KEYS: &str = include_str!("../cert-keys.txt");

#[derive(Debug, Deserialize)]
struct Payload {
    v: u8,
    kid: String,
    /// Numeric account id; certificates from before DECISION-0116 carry only this.
    #[serde(default)]
    sub: Option<u64>,
    /// Account id as a string (DECISION-0116), because providers other than GitHub do not use numbers.
    #[serde(default)]
    id: Option<String>,
    /// Short lowercase provider name (DECISION-0116); absent means `github`.
    #[serde(default)]
    provider: Option<String>,
    login: String,
    dpk: String,
    exp: u64,
    /// When the function issued it; certificates from before this field existed have none.
    #[serde(default)]
    iat: Option<u64>,
}

/// A verified certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceCert {
    /// Short lowercase provider name; `github` unless the certificate says otherwise.
    pub provider: String,
    /// The account id within `provider`; a string (DECISION-0116).
    pub account_id: String,
    pub login: String,
    /// Stable id of the device key, used by the revocation list.
    pub device: String,
    pub issued_at: Option<u64>,
    dpk: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CertError {
    Malformed,
    UnknownKid,
    BadSignature,
    Expired,
}

/// The function public keys this server trusts, by `kid`; several can be live while one rotates.
#[derive(Clone, Default)]
pub struct CertKeys(HashMap<String, EdKey>);

impl CertKeys {
    /// `TERMENBOX_CERT_KEYS` (comma or newline separated `kid=key`) replaces the built-in list, for
    /// self-hosted deployments of the client function (DECISION-0052).
    pub fn from_env() -> Result<Self> {
        match std::env::var("TERMENBOX_CERT_KEYS") {
            Ok(v) if !v.trim().is_empty() => Self::parse(&v),
            _ => Self::parse(DEFAULT_KEYS),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut keys = HashMap::new();
        let lines = text.lines().filter(|l| !l.trim_start().starts_with('#'));
        for entry in lines.flat_map(|l| l.split(',')).map(str::trim) {
            if entry.is_empty() {
                continue;
            }
            let (kid, key) = entry
                .split_once('=')
                .ok_or_else(|| anyhow!("bad certificate key entry: expected kid=key"))?;
            let bytes: [u8; 32] = B64
                .decode(key.trim())?
                .try_into()
                .map_err(|_| anyhow!("certificate key {kid} is not 32 bytes"))?;
            keys.insert(kid.trim().to_string(), EdKey::from_bytes(&bytes)?);
        }
        Ok(Self(keys))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Checks the signer, the expiry and the format; the device signature is checked separately.
    pub fn verify(&self, cert: &str, now_secs: u64) -> Result<DeviceCert, CertError> {
        let (body, sig) = cert.split_once('.').ok_or(CertError::Malformed)?;
        let body = B64.decode(body).map_err(|_| CertError::Malformed)?;
        let sig = B64.decode(sig).map_err(|_| CertError::Malformed)?;
        let p: Payload = serde_json::from_slice(&body).map_err(|_| CertError::Malformed)?;
        if p.v != 1 {
            return Err(CertError::Malformed);
        }
        let key = self.0.get(&p.kid).ok_or(CertError::UnknownKid)?;
        let sig = EdSignature::from_slice(&sig).map_err(|_| CertError::BadSignature)?;
        key.verify(&body, &sig)
            .map_err(|_| CertError::BadSignature)?;
        if p.exp <= now_secs {
            return Err(CertError::Expired);
        }
        if p.exp > now_secs + MAX_LIFETIME_SECS {
            return Err(CertError::Malformed);
        }
        let dpk = B64.decode(&p.dpk).map_err(|_| CertError::Malformed)?;
        P256Key::from_sec1_bytes(&dpk).map_err(|_| CertError::Malformed)?;
        // A certificate without a provider is GitHub; without an `id` its numeric `sub` is the id.
        let provider = p.provider.unwrap_or_else(|| "github".to_string());
        let account_id =
            p.id.or_else(|| p.sub.map(|n| n.to_string()))
                .ok_or(CertError::Malformed)?;
        Ok(DeviceCert {
            provider,
            account_id,
            login: p.login,
            device: device_id(&dpk),
            issued_at: p.iat,
            dpk,
        })
    }
}

/// Stable, public name of a device key: the first 16 bytes of its SHA-256.
pub fn device_id(dpk: &[u8]) -> String {
    B64.encode(&Sha256::digest(dpk)[..16])
}

/// What the device signs: bound to this server and to this connection.
pub fn challenge_message(server_id: &str, nonce: &str) -> Vec<u8> {
    format!("termenbox-auth-v1\n{server_id}\n{nonce}").into_bytes()
}

impl DeviceCert {
    /// Verifies the device's signature (WebCrypto's raw r||s form) over the challenge.
    pub fn verify_challenge(&self, server_id: &str, nonce: &str, signature: &str) -> Result<()> {
        let key = P256Key::from_sec1_bytes(&self.dpk)?;
        let sig = P256Signature::from_slice(&B64.decode(signature)?)?;
        if key
            .verify(&challenge_message(server_id, nonce), &sig)
            .is_err()
        {
            bail!("bad challenge signature");
        }
        Ok(())
    }
}

#[cfg(test)]
pub mod testing {
    //! Mints certificates the way the hosted function does, for tests.
    use super::*;
    use ed25519_dalek::{Signer as _, SigningKey};
    use p256::ecdsa::SigningKey as P256Signing;

    pub struct Signer {
        pub kid: String,
        key: SigningKey,
    }

    pub struct Device {
        key: P256Signing,
    }

    impl Signer {
        pub fn new(kid: &str) -> Self {
            Self {
                kid: kid.into(),
                key: SigningKey::generate(&mut rand::rngs::OsRng),
            }
        }

        pub fn keys(&self) -> CertKeys {
            CertKeys::parse(&format!(
                "{}={}",
                self.kid,
                B64.encode(self.key.verifying_key().to_bytes())
            ))
            .unwrap()
        }

        pub fn issue(&self, device: &Device, id: u64, login: &str, exp: u64) -> String {
            self.issue_at(device, id, login, exp, Some(now()))
        }

        /// Like the function's certificates, but with a chosen (or missing) issue time. Carries the
        /// DECISION-0116 `provider` and string `id` alongside the legacy numeric `sub`.
        pub fn issue_at(
            &self,
            device: &Device,
            id: u64,
            login: &str,
            exp: u64,
            iat: Option<u64>,
        ) -> String {
            let dpk = device.public();
            let mut body = serde_json::json!({
                "v": 1, "kid": self.kid, "sub": id, "id": id.to_string(), "provider": "github",
                "login": login, "dpk": B64.encode(dpk), "exp": exp,
            });
            if let Some(iat) = iat {
                body["iat"] = iat.into();
            }
            self.sign_body(body)
        }

        /// A certificate in the pre-0116 form: a numeric `sub`, no `provider` and no string `id`.
        pub fn issue_legacy_at(
            &self,
            device: &Device,
            id: u64,
            login: &str,
            exp: u64,
            iat: Option<u64>,
        ) -> String {
            let dpk = device.public();
            let mut body = serde_json::json!({
                "v": 1, "kid": self.kid, "sub": id, "login": login, "dpk": B64.encode(dpk), "exp": exp,
            });
            if let Some(iat) = iat {
                body["iat"] = iat.into();
            }
            self.sign_body(body)
        }

        fn sign_body(&self, body: serde_json::Value) -> String {
            let body = body.to_string();
            let sig = self.key.sign(body.as_bytes());
            format!("{}.{}", B64.encode(body), B64.encode(sig.to_bytes()))
        }
    }

    impl Device {
        pub fn new() -> Self {
            Self {
                key: P256Signing::random(&mut rand::rngs::OsRng),
            }
        }

        pub fn public(&self) -> Vec<u8> {
            self.key
                .verifying_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec()
        }

        pub fn id(&self) -> String {
            device_id(&self.public())
        }

        pub fn sign(&self, server_id: &str, nonce: &str) -> String {
            let sig: P256Signature = self.key.sign(&challenge_message(server_id, nonce));
            B64.encode(sig.to_bytes())
        }
    }

    pub fn now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    #[test]
    fn a_valid_certificate_and_challenge_signature_verify() {
        let (signer, device) = (Signer::new("k1"), Device::new());
        let cert = signer.issue(&device, 7, "octo", now() + 3600);
        let c = signer.keys().verify(&cert, now()).unwrap();
        assert_eq!(
            (c.provider.as_str(), c.account_id.as_str(), c.login.as_str()),
            ("github", "7", "octo")
        );
        assert_eq!(c.device, device.id());
        c.verify_challenge("srv", "n1", &device.sign("srv", "n1"))
            .unwrap();
    }

    #[test]
    fn an_old_form_certificate_is_read_as_github_with_the_numeric_sub_as_id() {
        let (signer, device) = (Signer::new("k1"), Device::new());
        let cert = signer.issue_legacy_at(&device, 42, "octo", now() + 3600, Some(now()));
        let c = signer.keys().verify(&cert, now()).unwrap();
        assert_eq!(
            (c.provider.as_str(), c.account_id.as_str(), c.login.as_str()),
            ("github", "42", "octo")
        );
    }

    #[test]
    fn a_signature_for_another_server_or_nonce_fails() {
        let (signer, device) = (Signer::new("k1"), Device::new());
        let c = signer
            .keys()
            .verify(&signer.issue(&device, 7, "o", now() + 60), now())
            .unwrap();
        let sig = device.sign("server-a", "n1");
        assert!(c.verify_challenge("server-b", "n1", &sig).is_err());
        assert!(c.verify_challenge("server-a", "n2", &sig).is_err());
        let other = Device::new();
        assert!(c
            .verify_challenge("server-a", "n1", &other.sign("server-a", "n1"))
            .is_err());
    }

    #[test]
    fn expired_unknown_and_forged_certificates_fail() {
        let (signer, device) = (Signer::new("k1"), Device::new());
        let keys = signer.keys();
        let expired = signer.issue(&device, 7, "o", now() - 1);
        assert_eq!(keys.verify(&expired, now()), Err(CertError::Expired));

        let stranger = Signer::new("other-kid");
        let unknown = stranger.issue(&device, 7, "o", now() + 60);
        assert_eq!(keys.verify(&unknown, now()), Err(CertError::UnknownKid));

        // Same kid, different key: the signature does not verify.
        let forger = Signer::new("k1");
        let forged = forger.issue(&device, 7, "o", now() + 60);
        assert_eq!(keys.verify(&forged, now()), Err(CertError::BadSignature));

        let long = signer.issue(&device, 7, "o", now() + 30 * 24 * 3600);
        assert_eq!(keys.verify(&long, now()), Err(CertError::Malformed));
        assert_eq!(keys.verify("garbage", now()), Err(CertError::Malformed));
    }

    #[test]
    fn the_issue_time_is_read_when_present_and_missing_in_old_certificates() {
        let (signer, device) = (Signer::new("k1"), Device::new());
        let keys = signer.keys();
        let with = signer.issue_at(&device, 7, "o", now() + 3600, Some(1234));
        assert_eq!(keys.verify(&with, now()).unwrap().issued_at, Some(1234));
        let without = signer.issue_at(&device, 7, "o", now() + 3600, None);
        assert_eq!(keys.verify(&without, now()).unwrap().issued_at, None);
    }

    #[test]
    fn the_built_in_key_list_parses() {
        assert!(!CertKeys::parse(DEFAULT_KEYS).unwrap().is_empty());
    }
}
