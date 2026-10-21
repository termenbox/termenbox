//! Candidate bundles (DECISION-0123): the addresses a server can be reached at, signed by the server key.
//!
//! An address is only a hint around one identity (the ed25519 key and the pinned DTLS fingerprint), so a bundle
//! is verified before any of it is previewed, connected to or cached (GUARDRAIL-connection-hints-are-signed-and-
//! never-identity). The signed bytes are an explicit layout, not JSON, so Rust and TypeScript agree byte for byte
//! (shared vectors in `testdata/bundle-vectors.json`; the TypeScript side is `packages/client/src/lib/bundle.ts`).
//!
//! Layout `tbxb1` (all integers big-endian):
//!
//! ```text
//! "tbxb1"                 5 bytes  magic and version
//! server_id               32 bytes ed25519 public key
//! seq                     u64      increases with every bundle the server signs
//! expires                 u64      unix seconds; a bundle past it is ignored
//! fingerprint             32 bytes SHA-256 of the server's DTLS certificate
//! name                    u8 length + UTF-8 (at most 64 bytes)
//! count                   u8       at most 16
//! count x { kind u8 (1 lan, 2 wan4, 3 wan6, 4 overlay, 5 tunnel), port u16, addr u8 length + UTF-8 }
//! ```
//!
//! The wire form is `base64url(signed bytes || signature (64 bytes))`: one string for the in-band push and the
//! pairing link alike.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD as B64, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};

pub const MAGIC: &[u8; 5] = b"tbxb1";
pub const MAX_CANDIDATES: usize = 16;
pub const MAX_NAME: usize = 64;
const SIG_LEN: usize = 64;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CandidateKind {
    Lan,
    Wan4,
    Wan6,
    Overlay,
    Tunnel,
}

impl CandidateKind {
    pub fn code(self) -> u8 {
        match self {
            Self::Lan => 1,
            Self::Wan4 => 2,
            Self::Wan6 => 3,
            Self::Overlay => 4,
            Self::Tunnel => 5,
        }
    }

    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            1 => Self::Lan,
            2 => Self::Wan4,
            3 => Self::Wan6,
            4 => Self::Overlay,
            5 => Self::Tunnel,
            _ => return None,
        })
    }

    /// Candidates the public directory must never carry (GUARDRAIL-lan-addresses-never-reach-the-directory).
    pub fn is_private(self) -> bool {
        matches!(self, Self::Lan | Self::Overlay)
    }
}

/// One way to reach the server: an IP (or, for a tunnel, a URL) and a port.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub kind: CandidateKind,
    pub addr: String,
    pub port: u16,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// The server's ed25519 public key, base64url (no padding).
    pub server_id: String,
    pub seq: u64,
    /// Unix seconds.
    pub expires: u64,
    /// `sha-256 AA:BB:...` as the directory record carries it.
    pub fingerprint: String,
    pub name: String,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BundleError {
    #[error("malformed bundle")]
    Malformed,
    #[error("bad signature")]
    BadSignature,
    #[error("bundle expired")]
    Expired,
    #[error("stale bundle (seq not above the stored one)")]
    Stale,
    #[error("bundle is for another server")]
    WrongServer,
}

fn parse_fingerprint(s: &str) -> Option<[u8; 32]> {
    let hex = s.strip_prefix("sha-256 ")?;
    let mut out = [0u8; 32];
    let mut parts = hex.split(':');
    for byte in out.iter_mut() {
        let p = parts.next()?;
        if p.len() != 2 {
            return None;
        }
        *byte = u8::from_str_radix(p, 16).ok()?;
    }
    parts.next().is_none().then_some(out)
}

fn format_fingerprint(bytes: &[u8]) -> String {
    let hex: Vec<String> = bytes.iter().map(|b| format!("{b:02X}")).collect();
    format!("sha-256 {}", hex.join(":"))
}

fn server_key_bytes(server_id: &str) -> Option<[u8; 32]> {
    B64.decode(server_id).ok()?.try_into().ok()
}

impl Bundle {
    /// The exact bytes that are signed. `Malformed` when a field does not fit the layout.
    pub fn signed_bytes(&self) -> Result<Vec<u8>, BundleError> {
        if self.candidates.len() > MAX_CANDIDATES || self.name.len() > MAX_NAME {
            return Err(BundleError::Malformed);
        }
        let mut out = MAGIC.to_vec();
        out.extend(server_key_bytes(&self.server_id).ok_or(BundleError::Malformed)?);
        out.extend(self.seq.to_be_bytes());
        out.extend(self.expires.to_be_bytes());
        out.extend(parse_fingerprint(&self.fingerprint).ok_or(BundleError::Malformed)?);
        out.push(self.name.len() as u8);
        out.extend(self.name.as_bytes());
        out.push(self.candidates.len() as u8);
        for c in &self.candidates {
            if c.addr.len() > 255 {
                return Err(BundleError::Malformed);
            }
            out.push(c.kind.code());
            out.extend(c.port.to_be_bytes());
            out.push(c.addr.len() as u8);
            out.extend(c.addr.as_bytes());
        }
        Ok(out)
    }

    /// The wire string for a signature made over `signed_bytes()`.
    pub fn encode(&self, signature: &[u8; 64]) -> Result<String, BundleError> {
        let mut bytes = self.signed_bytes()?;
        bytes.extend(signature);
        Ok(B64.encode(bytes))
    }

    /// Parses the layout without checking the signature.
    pub fn decode(wire: &str) -> Result<(Bundle, [u8; 64]), BundleError> {
        let bytes = B64.decode(wire.trim()).map_err(|_| BundleError::Malformed)?;
        if bytes.len() < MAGIC.len() + SIG_LEN {
            return Err(BundleError::Malformed);
        }
        let (body, sig) = bytes.split_at(bytes.len() - SIG_LEN);
        let mut r = Reader(body);
        if r.take(5)? != MAGIC {
            return Err(BundleError::Malformed);
        }
        let server_id = B64.encode(r.take(32)?);
        let seq = u64::from_be_bytes(r.take(8)?.try_into().unwrap());
        let expires = u64::from_be_bytes(r.take(8)?.try_into().unwrap());
        let fingerprint = format_fingerprint(r.take(32)?);
        let name_len = r.take(1)?[0] as usize;
        let name =
            String::from_utf8(r.take(name_len)?.to_vec()).map_err(|_| BundleError::Malformed)?;
        let count = r.take(1)?[0] as usize;
        if count > MAX_CANDIDATES || name.len() > MAX_NAME {
            return Err(BundleError::Malformed);
        }
        let mut candidates = Vec::new();
        for _ in 0..count {
            let kind = CandidateKind::from_code(r.take(1)?[0]).ok_or(BundleError::Malformed)?;
            let port = u16::from_be_bytes(r.take(2)?.try_into().unwrap());
            let len = r.take(1)?[0] as usize;
            let addr =
                String::from_utf8(r.take(len)?.to_vec()).map_err(|_| BundleError::Malformed)?;
            candidates.push(Candidate { kind, addr, port });
        }
        if !r.0.is_empty() {
            return Err(BundleError::Malformed);
        }
        let bundle = Bundle {
            server_id,
            seq,
            expires,
            fingerprint,
            name,
            candidates,
        };
        Ok((bundle, sig.try_into().unwrap()))
    }

    /// Decodes and verifies a wire bundle against the key it names. Nothing in it may be used before this passes.
    ///
    /// `pin` is the server id already stored for a known server (`WrongServer` when the bundle names another);
    /// `stored_seq` is the highest `seq` accepted before (`Stale` unless this one is higher). A first contact has
    /// neither: the signature then proves only that whoever made the link holds the key it carries.
    pub fn verify(
        wire: &str,
        now_secs: u64,
        pin: Option<&str>,
        stored_seq: Option<u64>,
    ) -> Result<Bundle, BundleError> {
        let (bundle, sig) = Self::decode(wire)?;
        if let Some(pin) = pin {
            if bundle.server_id != pin {
                return Err(BundleError::WrongServer);
            }
        }
        let key = VerifyingKey::from_bytes(
            &server_key_bytes(&bundle.server_id).ok_or(BundleError::Malformed)?,
        )
        .map_err(|_| BundleError::Malformed)?;
        key.verify(&bundle.signed_bytes()?, &Signature::from_bytes(&sig))
            .map_err(|_| BundleError::BadSignature)?;
        if bundle.expires <= now_secs {
            return Err(BundleError::Expired);
        }
        if stored_seq.is_some_and(|s| bundle.seq <= s) {
            return Err(BundleError::Stale);
        }
        Ok(bundle)
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], BundleError> {
        if self.0.len() < n {
            return Err(BundleError::Malformed);
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn vector_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn vector_bundle() -> Bundle {
        Bundle {
            server_id: B64.encode(vector_key().verifying_key().to_bytes()),
            seq: 41,
            expires: 1_900_000_000,
            fingerprint: format_fingerprint(&[0xAB; 32]),
            name: "home-box".into(),
            candidates: vec![
                Candidate {
                    kind: CandidateKind::Lan,
                    addr: "192.168.1.20".into(),
                    port: 4443,
                },
                Candidate {
                    kind: CandidateKind::Wan6,
                    addr: "2001:db8::20".into(),
                    port: 4443,
                },
                Candidate {
                    kind: CandidateKind::Tunnel,
                    addr: "wss://x.example/ws".into(),
                    port: 443,
                },
            ],
        }
    }

    fn signed(b: &Bundle) -> String {
        let sig = vector_key().sign(&b.signed_bytes().unwrap());
        b.encode(&sig.to_bytes()).unwrap()
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn bundle_signature_verifies() {
        let b = vector_bundle();
        assert_eq!(Bundle::verify(&signed(&b), 1_800_000_000, None, None), Ok(b));
    }

    /// The shared vector: the TypeScript client must produce and accept the same bytes.
    #[test]
    fn rust_and_typescript_agree_on_the_shared_vector() {
        let v: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/bundle-vectors.json")).unwrap();
        let b = vector_bundle();
        assert_eq!(v["server_id"], b.server_id);
        assert_eq!(v["signed_hex"], hex(&b.signed_bytes().unwrap()));
        assert_eq!(v["wire"], signed(&b));
        assert_eq!(
            serde_json::from_value::<Bundle>(v["bundle"].clone()).unwrap(),
            b
        );
    }

    #[test]
    fn tampered_bundle_fails() {
        let wire = signed(&vector_bundle());
        let bytes = B64.decode(&wire).unwrap();
        // Flip every bit in turn: address, port, seq, fingerprint, name and signature all fail.
        for i in 0..bytes.len() * 8 {
            let mut t = bytes.clone();
            t[i / 8] ^= 1 << (i % 8);
            assert!(
                Bundle::verify(&B64.encode(&t), 1_800_000_000, None, None).is_err(),
                "bit {i} went unnoticed"
            );
        }
    }

    #[test]
    fn stale_seq_rejected() {
        let wire = signed(&vector_bundle());
        assert_eq!(Bundle::verify(&wire, 1, None, Some(41)), Err(BundleError::Stale));
        assert_eq!(Bundle::verify(&wire, 1, None, Some(99)), Err(BundleError::Stale));
        assert!(Bundle::verify(&wire, 1, None, Some(40)).is_ok());
    }

    #[test]
    fn expired_and_wrong_server_are_refused() {
        let wire = signed(&vector_bundle());
        assert_eq!(
            Bundle::verify(&wire, 1_900_000_000, None, None),
            Err(BundleError::Expired)
        );
        let other = B64.encode(SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes());
        assert_eq!(
            Bundle::verify(&wire, 1, Some(&other), None),
            Err(BundleError::WrongServer)
        );
    }

    #[test]
    fn a_bundle_signed_by_another_key_is_refused() {
        let b = vector_bundle();
        let sig = SigningKey::from_bytes(&[9u8; 32]).sign(&b.signed_bytes().unwrap());
        let wire = b.encode(&sig.to_bytes()).unwrap();
        assert_eq!(Bundle::verify(&wire, 1, None, None), Err(BundleError::BadSignature));
    }

    #[test]
    fn garbage_is_malformed_not_a_panic() {
        for s in ["", "AAAA", "tbxb1", &B64.encode(b"tbxb1"), &"A".repeat(400)] {
            assert!(Bundle::decode(s).is_err(), "{s}");
        }
    }

    #[test]
    fn oversized_fields_do_not_encode() {
        let mut b = vector_bundle();
        b.name = "x".repeat(MAX_NAME + 1);
        assert_eq!(b.signed_bytes(), Err(BundleError::Malformed));
        let mut b = vector_bundle();
        b.candidates = vec![b.candidates[0].clone(); MAX_CANDIDATES + 1];
        assert_eq!(b.signed_bytes(), Err(BundleError::Malformed));
    }

    /// Run with `--ignored --nocapture` to regenerate `testdata/bundle-vectors.json`.
    #[test]
    #[ignore]
    fn print_vectors() {
        let b = vector_bundle();
        let v = serde_json::json!({
            "server_id": b.server_id,
            "bundle": b,
            "signed_hex": hex(&b.signed_bytes().unwrap()),
            "wire": signed(&b),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
    }
}
