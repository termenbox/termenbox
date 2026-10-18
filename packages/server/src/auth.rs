use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::cert::{CertError, CertKeys};

/// A claim secret works for this long after it is issued, then once only.
const CLAIM_TTL: Duration = Duration::from_secs(15 * 60);

/// A user, as vouched for by a device certificate: a provider and an account id within it (DECISION-0116).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Short lowercase provider name; `owner.json` written before 0116 has none and reads as `github`.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// Account id within `provider`, a string because other providers do not use numbers.
    #[serde(deserialize_with = "string_or_number")]
    pub id: String,
    pub login: String,
    /// Issue time of the certificate this connection used; never written to owner.json.
    #[serde(skip)]
    pub issued_at: Option<u64>,
}

fn default_provider() -> String {
    "github".to_string()
}

/// Reads an id written either as a JSON number (GitHub before DECISION-0116) or as a string. Any JSON
/// number is accepted, so a hand-edited value does not make `owner.json` unreadable.
fn string_or_number<'de, D>(de: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Id {
        Num(serde_json::Number),
        Text(String),
    }
    Ok(match Id::deserialize(de)? {
        Id::Num(n) => n.to_string(),
        Id::Text(s) => s,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    /// The user is the owner (already bound, or bound just now with the claim token).
    Allowed,
    /// The server is unclaimed and the claim token is missing or wrong.
    NeedsClaim,
    /// The server belongs to someone else.
    Denied,
}

/// Verifies device certificates and decides who may use this server (DECISION-0062, DECISION-0053).
/// The server holds no secret of the hosted function: it checks signatures against its public keys.
pub struct AuthState {
    dir: PathBuf,
    keys: CertKeys,
    server_id: String,
    // Serialises owner binding so two clients cannot both claim.
    bind_lock: Mutex<()>,
    claim_ttl: Duration,
}

/// What `owner.json` says: someone owns the server, nobody does yet, or the file is there but cannot be
/// read. The last case must never look unclaimed, or a new claim could take the server over (a downgrade
/// that writes a string id, or a corrupted file, both land here).
enum OwnerFile {
    None,
    Owned(Identity),
    Unreadable,
}

impl AuthState {
    pub fn new(dir: PathBuf, keys: CertKeys, server_id: String) -> Result<Self> {
        std::fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            keys,
            server_id,
            bind_lock: Mutex::new(()),
            claim_ttl: CLAIM_TTL,
        })
    }

    pub fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    fn owner_path(&self) -> PathBuf {
        self.dir.join("owner.json")
    }

    fn claim_path(&self) -> PathBuf {
        self.dir.join("claim_token")
    }

    /// The owner file, told apart from "no owner yet".
    fn read_owner(&self) -> OwnerFile {
        match std::fs::read_to_string(self.owner_path()) {
            Ok(text) => match serde_json::from_str(&text) {
                Ok(owner) => OwnerFile::Owned(owner),
                Err(e) => {
                    warn!(target: "audit", event = "owner_unreadable", error = %e);
                    OwnerFile::Unreadable
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => OwnerFile::None,
            Err(e) => {
                warn!(target: "audit", event = "owner_unreadable", error = %e);
                OwnerFile::Unreadable
            }
        }
    }

    pub fn owner(&self) -> Option<Identity> {
        match self.read_owner() {
            OwnerFile::Owned(owner) => Some(owner),
            _ => None,
        }
    }

    /// True when the server has an owner, or an owner file we cannot read. Security decisions treat an
    /// unreadable file as claimed, never as free to claim again.
    pub fn is_claimed(&self) -> bool {
        !matches!(self.read_owner(), OwnerFile::None)
    }

    #[cfg(test)]
    pub fn set_claim_ttl(&mut self, ttl: Duration) {
        self.claim_ttl = ttl;
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// The unexpired claim secret, if the server is unclaimed. Stored as `secret\nexpires_ms`.
    pub fn active_claim(&self) -> Option<String> {
        if self.is_claimed() {
            return None;
        }
        let text = std::fs::read_to_string(self.claim_path()).ok()?;
        let (secret, expires) = text.split_once('\n')?;
        let expires: u64 = expires.trim().parse().ok()?;
        (!secret.is_empty() && Self::now_ms() < expires).then(|| secret.to_string())
    }

    fn store_claim(&self, secret: &str) -> Result<()> {
        let expires = Self::now_ms() + self.claim_ttl.as_millis() as u64;
        write_private(
            &self.claim_path(),
            format!("{secret}\n{expires}").as_bytes(),
        )
    }

    /// Uses a secret chosen by the client (from the install command) as the claim token.
    pub fn provide_claim(&self, secret: &str) -> Result<()> {
        if !self.is_claimed() && !secret.is_empty() {
            self.store_claim(secret)?;
        }
        Ok(())
    }

    /// Root-proven re-claim (the installer ran again with a new secret): forget the owner and start a
    /// new 15-minute claim. Audit-logged without the secret.
    pub fn reclaim(&self, secret: &str) -> Result<()> {
        if secret.is_empty() {
            return Err(anyhow!("empty claim secret"));
        }
        crate::uninstall::clear(&self.dir);
        match self.read_owner() {
            OwnerFile::Owned(owner) => {
                info!(target: "audit", event = "owner_reset", provider = %owner.provider, account_id = %owner.id);
                let _ = std::fs::remove_file(self.owner_path());
            }
            OwnerFile::Unreadable => {
                info!(target: "audit", event = "owner_reset", reason = "unreadable_owner_file");
                let _ = std::fs::remove_file(self.owner_path());
            }
            OwnerFile::None => {}
        }
        self.store_claim(secret)
    }

    /// The claim token of an unclaimed server, made fresh when none is valid. `None` once claimed.
    pub fn claim_token(&self) -> Result<Option<String>> {
        if self.is_claimed() {
            return Ok(None);
        }
        if let Some(secret) = self.active_claim() {
            return Ok(Some(secret));
        }
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        let token = URL_SAFE_NO_PAD.encode(bytes);
        self.store_claim(&token)?;
        Ok(Some(token))
    }

    /// Decides whether `identity` may use the server; binds the first user who presents the claim token.
    pub fn authorize(&self, identity: &Identity, claim: Option<&str>) -> Result<Decision> {
        let _guard = self.bind_lock.lock().unwrap();
        match self.read_owner() {
            OwnerFile::Owned(owner) => {
                return Ok(
                    if owner.provider == identity.provider && owner.id == identity.id {
                        Decision::Allowed
                    } else {
                        Decision::Denied
                    },
                );
            }
            OwnerFile::Unreadable => {
                return Err(anyhow!(
                    "owner.json exists but cannot be read; refusing to treat the server as unclaimed"
                ));
            }
            OwnerFile::None => {}
        }
        let expected = self
            .claim_token()?
            .ok_or_else(|| anyhow!("no claim token"))?;
        match claim {
            Some(c) if constant_time_eq(c.as_bytes(), expected.as_bytes()) => {
                write_private(
                    &self.owner_path(),
                    serde_json::to_string(identity)?.as_bytes(),
                )?;
                let _ = std::fs::remove_file(self.claim_path());
                info!(target: "audit", event = "server_claimed", provider = %identity.provider, account_id = %identity.id, login = %identity.login);
                Ok(Decision::Allowed)
            }
            _ => Ok(Decision::NeedsClaim),
        }
    }

    fn revoked_path(&self) -> PathBuf {
        self.dir.join("revoked_devices.json")
    }

    pub fn revoked_devices(&self) -> Vec<String> {
        std::fs::read_to_string(self.revoked_path())
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Adds a device to the revocation list (idempotent). Only the owner reaches this.
    pub fn revoke_device(&self, device: &str) -> Result<()> {
        let _guard = self.bind_lock.lock().unwrap();
        let mut list = self.revoked_devices();
        if !list.iter().any(|d| d == device) {
            list.push(device.to_string());
            write_private(
                &self.revoked_path(),
                serde_json::to_string(&list)?.as_bytes(),
            )?;
            info!(target: "audit", event = "device_revoked", device);
        }
        Ok(())
    }

    /// Full check for one connection: certificate, challenge signature, revocation, ownership. Writes the
    /// audit record (never the certificate or signature).
    pub fn check(
        &self,
        cert: &str,
        signature: &str,
        nonce: &str,
        claim: Option<&str>,
        ip: Option<&str>,
    ) -> Result<Identity, AuthError> {
        let ip = ip.unwrap_or("-");
        let fail = |reason: &'static str, e: AuthError| {
            warn!(target: "audit", event = "auth", success = false, reason, ip);
            e
        };
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let device = self.keys.verify(cert, now).map_err(|e| match e {
            CertError::Expired => fail("expired_certificate", AuthError::Expired),
            CertError::UnknownKid => fail("unknown_kid", AuthError::BadCertificate),
            CertError::BadSignature => fail("bad_certificate_signature", AuthError::BadCertificate),
            CertError::Malformed => fail("malformed_certificate", AuthError::BadCertificate),
        })?;
        if device
            .verify_challenge(&self.server_id, nonce, signature)
            .is_err()
        {
            return Err(fail("bad_challenge_signature", AuthError::BadCertificate));
        }
        if self.revoked_devices().contains(&device.device) {
            return Err(fail("device_revoked", AuthError::Revoked));
        }
        let identity = Identity {
            provider: device.provider,
            id: device.account_id,
            login: device.login.clone(),
            issued_at: device.issued_at,
        };
        let decision = self
            .authorize(&identity, claim)
            .map_err(|_| AuthError::Internal)?;
        let (success, reason) = match decision {
            Decision::Allowed => (true, "ok"),
            Decision::NeedsClaim => (false, "needs_claim"),
            Decision::Denied => (false, "not_owner"),
        };
        info!(target: "audit", event = "auth", success, reason, provider = %identity.provider, account_id = %identity.id, login = %identity.login, device = %device.device, ip);
        match decision {
            Decision::Allowed => Ok(identity),
            Decision::NeedsClaim => Err(AuthError::NeedsClaim),
            Decision::Denied => Err(AuthError::NotOwner),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthError {
    BadCertificate,
    Expired,
    Revoked,
    NeedsClaim,
    NotOwner,
    Internal,
}

impl AuthError {
    pub fn message(&self) -> &'static str {
        match self {
            AuthError::BadCertificate => "This device's login is invalid; sign in again",
            AuthError::Expired => "Your login has expired; sign in again",
            AuthError::Revoked => "This device has been revoked",
            AuthError::NeedsClaim => {
                "This server is not registered yet; add it with its registration link"
            }
            AuthError::NotOwner => "This server belongs to another user",
            AuthError::Internal => "Authentication failed",
        }
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn write_private(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    opts.mode(0o600);
    opts.open(path)?.write_all(bytes)?;
    Ok(())
}

/// What `--register` shows (as text and as a QR): opening it, with the camera app or by scanning in the
/// client, starts Add Server with this secret. The fragment never reaches any server.
pub fn claim_link(client_url: &str, claim: &str) -> String {
    format!("{}/#claim={}", client_url.trim_end_matches('/'), claim)
}

/// Link that adds an already-registered server to the owner's browser. The server id is a public key, so
/// the link carries no secret; connecting still requires the owner's GitHub login.
pub fn server_link(client_url: &str, server_id: &str) -> String {
    format!("{}/#server={}", client_url.trim_end_matches('/'), server_id)
}

/// The same link as a terminal QR code: modules drawn light-on-dark for dark terminals; the client's
/// decoder tries both polarities.
pub fn claim_qr(link: &str) -> Option<String> {
    use qrcode::render::unicode::Dense1x2;
    let code = qrcode::QrCode::new(link.as_bytes()).ok()?;
    Some(
        code.render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("termenbox-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn id(n: u64) -> Identity {
        Identity {
            provider: "github".into(),
            id: n.to_string(),
            login: format!("user{n}"),
            issued_at: None,
        }
    }

    fn other_provider(provider: &str, id: &str) -> Identity {
        Identity {
            provider: provider.into(),
            id: id.into(),
            login: format!("{provider}{id}"),
            issued_at: None,
        }
    }

    fn auth() -> AuthState {
        AuthState::new(temp_dir(), CertKeys::default(), "srv".into()).unwrap()
    }

    #[test]
    fn first_claim_binds_owner_and_removes_token() {
        let a = auth();
        let claim = a.claim_token().unwrap().unwrap();
        assert_eq!(
            a.authorize(&id(1), Some(&claim)).unwrap(),
            Decision::Allowed
        );
        let owner = a.owner().unwrap();
        assert_eq!(
            (owner.provider.as_str(), owner.id.as_str()),
            ("github", "1")
        );
        assert!(a.claim_token().unwrap().is_none());
    }

    #[test]
    fn an_owner_file_without_a_provider_reads_as_github_and_still_opens_the_server() {
        let dir = temp_dir();
        // The shape the previous version wrote: a numeric id and no provider.
        std::fs::write(dir.join("owner.json"), r#"{"id": 9, "login": "legacy"}"#).unwrap();
        let a = AuthState::new(dir, CertKeys::default(), "srv".into()).unwrap();
        let owner = a.owner().unwrap();
        assert_eq!(
            (
                owner.provider.as_str(),
                owner.id.as_str(),
                owner.login.as_str()
            ),
            ("github", "9", "legacy")
        );
        assert_eq!(a.authorize(&id(9), None).unwrap(), Decision::Allowed);
        assert_eq!(a.authorize(&id(10), None).unwrap(), Decision::Denied);
    }

    #[test]
    fn an_unreadable_owner_file_is_treated_as_claimed_not_free() {
        let dir = temp_dir();
        std::fs::write(dir.join("owner.json"), "{ this is not json").unwrap();
        let a = AuthState::new(dir, CertKeys::default(), "srv".into()).unwrap();
        assert!(a.owner().is_none(), "there is no owner we can name");
        assert!(a.is_claimed(), "but the file must not look unclaimed");
        assert!(a.claim_token().unwrap().is_none(), "no new claim link");
        assert!(a.active_claim().is_none());
        a.provide_claim("one-time").unwrap();
        assert!(a.active_claim().is_none(), "a secret cannot claim over it");
        assert!(a.authorize(&id(1), None).is_err(), "login fails closed");
    }

    #[test]
    fn a_root_reclaim_resets_an_unreadable_owner_file() {
        let dir = temp_dir();
        std::fs::write(dir.join("owner.json"), "{ this is not json").unwrap();
        let a = AuthState::new(dir, CertKeys::default(), "srv".into()).unwrap();
        a.reclaim("fresh-secret").unwrap();
        assert!(!a.is_claimed());
        assert_eq!(a.active_claim().as_deref(), Some("fresh-secret"));
        assert_eq!(
            a.authorize(&id(3), Some("fresh-secret")).unwrap(),
            Decision::Allowed
        );
    }

    #[test]
    fn a_hand_edited_numeric_owner_id_still_reads() {
        let negative: Identity = serde_json::from_str(r#"{"id": -1, "login": "x"}"#).unwrap();
        assert_eq!(
            (negative.provider.as_str(), negative.id.as_str()),
            ("github", "-1")
        );
        let text: Identity =
            serde_json::from_str(r#"{"provider":"gitlab","id":"abc","login":"x"}"#).unwrap();
        assert_eq!(
            (text.provider.as_str(), text.id.as_str()),
            ("gitlab", "abc")
        );
    }

    #[test]
    fn the_same_id_from_another_provider_is_denied() {
        let a = auth();
        let claim = a.claim_token().unwrap().unwrap();
        a.authorize(&id(1), Some(&claim)).unwrap();
        let other = other_provider("gitlab", "1");
        assert_eq!(a.authorize(&other, None).unwrap(), Decision::Denied);
        assert_eq!(
            a.authorize(&other, Some(&claim)).unwrap(),
            Decision::Denied,
            "a spent claim cannot rebind the owner"
        );
    }

    #[test]
    fn owner_is_allowed_without_claim_and_others_denied() {
        let a = auth();
        let claim = a.claim_token().unwrap().unwrap();
        a.authorize(&id(1), Some(&claim)).unwrap();
        assert_eq!(a.authorize(&id(1), None).unwrap(), Decision::Allowed);
        assert_eq!(a.authorize(&id(2), None).unwrap(), Decision::Denied);
        assert_eq!(a.authorize(&id(2), Some(&claim)).unwrap(), Decision::Denied);
    }

    #[test]
    fn unclaimed_server_rejects_missing_or_wrong_claim() {
        let a = auth();
        let _ = a.claim_token().unwrap();
        assert_eq!(a.authorize(&id(1), None).unwrap(), Decision::NeedsClaim);
        assert_eq!(
            a.authorize(&id(1), Some("nope")).unwrap(),
            Decision::NeedsClaim
        );
        assert!(a.owner().is_none());
    }

    #[test]
    fn a_client_supplied_secret_claims_the_server_until_it_expires() {
        let a = auth();
        a.provide_claim("from-the-install-command").unwrap();
        assert_eq!(
            a.active_claim().as_deref(),
            Some("from-the-install-command")
        );
        assert_eq!(
            a.authorize(&id(1), Some("from-the-install-command"))
                .unwrap(),
            Decision::Allowed
        );
        assert!(a.active_claim().is_none(), "spent");

        let mut b = auth();
        b.set_claim_ttl(Duration::from_millis(1));
        b.provide_claim("late").unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(b.active_claim().is_none());
        assert_eq!(
            b.authorize(&id(1), Some("late")).unwrap(),
            Decision::NeedsClaim,
            "expired secrets are refused"
        );
    }

    #[test]
    fn a_root_proven_reclaim_replaces_the_owner() {
        let a = auth();
        let first = a.claim_token().unwrap().unwrap();
        a.authorize(&id(1), Some(&first)).unwrap();
        assert_eq!(a.authorize(&id(2), None).unwrap(), Decision::Denied);

        a.reclaim("new-device-secret").unwrap();
        assert!(a.owner().is_none());
        assert_eq!(
            a.authorize(&id(2), Some("new-device-secret")).unwrap(),
            Decision::Allowed
        );
        assert_eq!(
            a.authorize(&id(1), None).unwrap(),
            Decision::Denied,
            "the old owner is gone"
        );
        assert!(a.reclaim("").is_err());
    }

    #[test]
    fn claim_token_is_stable_until_used() {
        let a = auth();
        assert_eq!(a.claim_token().unwrap(), a.claim_token().unwrap());
    }

    #[test]
    fn owner_survives_restart() {
        let dir = temp_dir();
        let a = AuthState::new(dir.clone(), CertKeys::default(), "srv".into()).unwrap();
        let claim = a.claim_token().unwrap().unwrap();
        a.authorize(&id(7), Some(&claim)).unwrap();
        let b = AuthState::new(dir, CertKeys::default(), "srv".into()).unwrap();
        assert_eq!(b.authorize(&id(7), None).unwrap(), Decision::Allowed);
        assert_eq!(b.authorize(&id(8), None).unwrap(), Decision::Denied);
    }

    #[test]
    fn the_claim_link_carries_only_the_secret_in_the_fragment() {
        assert_eq!(
            claim_link("https://app.example.dev/", "tok"),
            "https://app.example.dev/#claim=tok"
        );
    }

    #[test]
    fn the_server_link_carries_only_the_public_id() {
        assert_eq!(
            server_link("https://app.example.dev/", "abc"),
            "https://app.example.dev/#server=abc"
        );
    }

    #[test]
    fn the_qr_encodes_the_link_and_fits_a_terminal() {
        let link = claim_link("https://app.termenbox.dev", &"A".repeat(43));
        let qr = claim_qr(&link).unwrap();
        let widest = qr.lines().map(|l| l.chars().count()).max().unwrap();
        assert!(widest <= 60, "QR is {widest} columns wide");
        assert!(qr.lines().count() <= 32);
    }

    #[test]
    fn check_verifies_the_certificate_and_enforces_ownership() {
        use crate::cert::testing::*;
        let signer = Signer::new("k1");
        let a = AuthState::new(temp_dir(), signer.keys(), "srv".into()).unwrap();
        let (alice, bob) = (Device::new(), Device::new());
        let cert = |d: &Device, id: u64, login: &str| signer.issue(d, id, login, now() + 3600);
        let check = |d: &Device, c: &str, nonce: &str, claim: Option<&str>| {
            a.check(c, &d.sign("srv", nonce), nonce, claim, None)
        };

        let ca = cert(&alice, 42, "octo");
        assert_eq!(
            check(&alice, &ca, "n", None).unwrap_err(),
            AuthError::NeedsClaim
        );
        let claim = a.claim_token().unwrap().unwrap();
        assert_eq!(check(&alice, &ca, "n", Some(&claim)).unwrap().id, "42");
        assert_eq!(check(&alice, &ca, "n2", None).unwrap().login, "octo");

        let cb = cert(&bob, 43, "other");
        assert_eq!(
            check(&bob, &cb, "n", None).unwrap_err(),
            AuthError::NotOwner
        );
        // Bob's key with Alice's certificate: the challenge signature does not match the certificate.
        assert_eq!(
            check(&bob, &ca, "n", None).unwrap_err(),
            AuthError::BadCertificate
        );
        // A signature made for another server does not work here.
        assert_eq!(
            a.check(&ca, &alice.sign("elsewhere", "n"), "n", None, None)
                .unwrap_err(),
            AuthError::BadCertificate
        );
    }

    #[test]
    fn an_old_form_certificate_authorises_as_github() {
        use crate::cert::testing::*;
        let signer = Signer::new("k1");
        let a = AuthState::new(temp_dir(), signer.keys(), "srv".into()).unwrap();
        let d = Device::new();
        let claim = a.claim_token().unwrap().unwrap();
        let legacy = signer.issue_legacy_at(&d, 7, "me", now() + 3600, Some(now()));
        let identity = a
            .check(&legacy, &d.sign("srv", "n"), "n", Some(&claim), None)
            .unwrap();
        assert_eq!(
            (identity.provider.as_str(), identity.id.as_str()),
            ("github", "7")
        );
    }

    #[test]
    fn expired_and_revoked_devices_are_refused_and_revocation_persists() {
        use crate::cert::testing::*;
        let signer = Signer::new("k1");
        let dir = temp_dir();
        let a = AuthState::new(dir.clone(), signer.keys(), "srv".into()).unwrap();
        let d = Device::new();
        let claim = a.claim_token().unwrap().unwrap();
        let good = signer.issue(&d, 1, "me", now() + 3600);
        a.check(&good, &d.sign("srv", "n"), "n", Some(&claim), None)
            .unwrap();

        let old = signer.issue(&d, 1, "me", now() - 5);
        assert_eq!(
            a.check(&old, &d.sign("srv", "n"), "n", None, None)
                .unwrap_err(),
            AuthError::Expired
        );

        a.revoke_device(&d.id()).unwrap();
        a.revoke_device(&d.id()).unwrap();
        let b = AuthState::new(dir, signer.keys(), "srv".into()).unwrap();
        assert_eq!(b.revoked_devices(), vec![d.id()]);
        assert_eq!(
            b.check(&good, &d.sign("srv", "n"), "n", None, None)
                .unwrap_err(),
            AuthError::Revoked
        );
    }
}
