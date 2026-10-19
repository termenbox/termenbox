//! Signed release manifests (decision 0076, guardrail updates-are-verified-and-reversible). CI signs
//! `manifest.json` with an Ed25519 release key in the OpenSSH signature format, so a host can check it with
//! plain `ssh-keygen -Y verify` before it trusts anything it downloaded (the installer), and the server can check
//! the same signature with the key embedded in this binary (the updater). The download host is never trusted.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};

/// The `ssh-keygen -Y sign -n` namespace; a signature made for anything else is refused.
pub const NAMESPACE: &str = "termenbox-release";
/// Release assets are only fetched from this project's GitHub releases.
pub const URL_PREFIX: &str = "https://github.com/termenbox/termenbox/releases/download/";
const DEFAULT_KEYS: &str = include_str!("../release-keys.txt");

/// The trusted release public keys: `kid ssh-ed25519 <base64 blob>` per line.
#[derive(Clone, Default)]
pub struct ReleaseKeys(Vec<(String, [u8; 32])>);

impl ReleaseKeys {
    /// `TERMENBOX_RELEASE_KEYS` replaces the built-in list (tests, forks that publish their own releases).
    pub fn from_env() -> Result<Self> {
        match std::env::var("TERMENBOX_RELEASE_KEYS") {
            Ok(v) if !v.trim().is_empty() => Self::parse(&v),
            _ => Self::parse(DEFAULT_KEYS),
        }
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut keys = vec![];
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let mut parts = line.split_whitespace();
            let (kid, kind, blob) = (parts.next(), parts.next(), parts.next());
            let (Some(kid), Some("ssh-ed25519"), Some(blob)) = (kid, kind, blob) else {
                bail!("bad release key line: expected `kid ssh-ed25519 <key>`");
            };
            keys.push((kid.to_string(), ed25519_from_blob(&STANDARD.decode(blob)?)?));
        }
        Ok(Self(keys))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn find(&self, key: &[u8; 32]) -> Option<&str> {
        self.0
            .iter()
            .find(|(_, k)| k == key)
            .map(|(kid, _)| kid.as_str())
    }
}

/// A cursor over SSH wire-format fields (big-endian u32 length, then bytes).
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.0.len() < n {
            bail!("truncated signature");
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn string(&mut self) -> Result<&'a [u8]> {
        let n = u32::from_be_bytes(self.take(4)?.try_into()?) as usize;
        self.take(n)
    }
}

fn ed25519_from_blob(blob: &[u8]) -> Result<[u8; 32]> {
    let mut r = Reader(blob);
    if r.string()? != b"ssh-ed25519" {
        bail!("not an ssh-ed25519 key");
    }
    r.string()?
        .try_into()
        .map_err(|_| anyhow!("an ed25519 key is 32 bytes"))
}

fn put(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

/// Checks an `ssh-keygen -Y sign` signature over `message` against the trusted keys and returns the key id.
pub fn verify_signature(message: &[u8], armored: &str, keys: &ReleaseKeys) -> Result<String> {
    let body: String = armored
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("-----") && !l.is_empty())
        .collect();
    let blob = STANDARD
        .decode(body)
        .map_err(|_| anyhow!("the signature is not valid base64"))?;
    let mut r = Reader(&blob);
    if r.take(6)? != b"SSHSIG" {
        bail!("not an SSH signature");
    }
    if u32::from_be_bytes(r.take(4)?.try_into()?) != 1 {
        bail!("unsupported SSH signature version");
    }
    let public = ed25519_from_blob(r.string()?)?;
    let namespace = r.string()?;
    let reserved = r.string()?;
    let hash_algorithm = r.string()?;
    let signature_blob = r.string()?;
    if namespace != NAMESPACE.as_bytes() {
        bail!("the signature was made for another purpose");
    }
    let digest: Vec<u8> = match hash_algorithm {
        b"sha512" => Sha512::digest(message).to_vec(),
        b"sha256" => Sha256::digest(message).to_vec(),
        _ => bail!("unsupported hash algorithm in the signature"),
    };
    let mut sr = Reader(signature_blob);
    if sr.string()? != b"ssh-ed25519" {
        bail!("not an ed25519 signature");
    }
    let signature =
        Signature::from_slice(sr.string()?).map_err(|_| anyhow!("bad signature length"))?;

    let kid = keys
        .find(&public)
        .ok_or_else(|| anyhow!("the signing key is not a trusted release key"))?;
    let mut signed = b"SSHSIG".to_vec();
    put(&mut signed, namespace);
    put(&mut signed, reserved);
    put(&mut signed, hash_algorithm);
    put(&mut signed, &digest);
    VerifyingKey::from_bytes(&public)?
        .verify(&signed, &signature)
        .map_err(|_| anyhow!("the signature does not match the manifest"))?;
    Ok(kid.to_string())
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Binary {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub version: String,
    /// A signed stop sign: while true no host updates or installs this release line (decision 0063).
    #[serde(default)]
    pub freeze: bool,
    #[serde(default)]
    pub notes: String,
    /// By architecture name (`x86_64`, `aarch64`).
    pub binaries: BTreeMap<String, Binary>,
    /// The bundled tmux by architecture (decision 0119). Absent in releases from before it was bundled.
    #[serde(default)]
    pub tmux: BTreeMap<String, Binary>,
    /// Licence notices that travel with the bundled tmux, by file name.
    #[serde(default)]
    pub tmux_notices: BTreeMap<String, Binary>,
}

impl Manifest {
    /// Verifies the signature first, then parses and validates; nothing in an unverified manifest is used.
    pub fn verify(
        bytes: &[u8],
        signature: &str,
        keys: &ReleaseKeys,
        url_prefix: &str,
    ) -> Result<(Manifest, String)> {
        let kid = verify_signature(bytes, signature, keys)?;
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|e| anyhow!("bad manifest: {e}"))?;
        parse_version(&manifest.version)?;
        let all = manifest
            .binaries
            .iter()
            .map(|(k, b)| (format!("{k} binary"), b))
            .chain(manifest.tmux.iter().map(|(k, b)| (format!("{k} tmux"), b)))
            .chain(
                manifest
                    .tmux_notices
                    .iter()
                    .map(|(k, b)| (format!("{k} notice"), b)),
            );
        for (what, b) in all {
            if !b.url.starts_with(url_prefix) || b.url.contains("..") {
                bail!("the {what} is not hosted by this project's releases");
            }
            if b.sha256.len() != 64 || !b.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                bail!("the {what} hash is not a SHA-256");
            }
        }
        Ok((manifest, kid))
    }

    pub fn binary(&self, arch: &str) -> Result<&Binary> {
        self.binaries
            .get(arch)
            .ok_or_else(|| anyhow!("this release has no {arch} binary"))
    }

    /// The bundled tmux for `arch`, when this release carries one.
    pub fn tmux(&self, arch: &str) -> Option<&Binary> {
        self.tmux.get(arch)
    }

    /// Whether this release is newer than the running `current` version.
    pub fn is_newer_than(&self, current: &str) -> Result<bool> {
        Ok(parse_version(&self.version)? > parse_version(current)?)
    }
}

fn parse_version(v: &str) -> Result<(u64, u64, u64)> {
    let core = v.split(['+', '-']).next().unwrap_or(v);
    let mut it = core.split('.').map(|p| p.parse::<u64>());
    match (it.next(), it.next(), it.next(), it.next()) {
        (Some(Ok(a)), Some(Ok(b)), Some(Ok(c)), None) => Ok((a, b, c)),
        _ => bail!("`{v}` is not a version like 1.2.3"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const PREFIX: &str = URL_PREFIX;

    fn manifest_json(version: &str, url: &str) -> String {
        format!(
            r#"{{"version":"{version}","freeze":false,"notes":"","binaries":{{"x86_64":{{"url":"{url}","sha256":"{}"}}}}}}"#,
            "a".repeat(64)
        )
    }

    /// Signs `manifest` with a throwaway key through the real `ssh-keygen`, the way CI does. None when absent.
    fn sign_with_ssh_keygen(manifest: &str, namespace: &str) -> Option<(ReleaseKeys, String)> {
        let dir = std::env::temp_dir().join(format!("termenbox-release-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).ok()?;
        let key = dir.join("key");
        let gen = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C", "t", "-f"])
            .arg(&key)
            .status()
            .ok()?;
        if !gen.success() {
            return None;
        }
        let file = dir.join("manifest.json");
        std::fs::write(&file, manifest).ok()?;
        let sign = Command::new("ssh-keygen")
            .args(["-Y", "sign", "-n", namespace, "-f"])
            .arg(&key)
            .arg(&file)
            .output()
            .ok()?;
        if !sign.status.success() {
            return None;
        }
        let public = std::fs::read_to_string(dir.join("key.pub")).ok()?;
        let line = format!(
            "r1 {}",
            public
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ")
        );
        Some((
            ReleaseKeys::parse(&line).ok()?,
            std::fs::read_to_string(dir.join("manifest.json.sig")).ok()?,
        ))
    }

    #[test]
    fn a_real_ssh_keygen_signature_verifies_and_any_change_breaks_it() {
        let m = manifest_json("1.2.3", &format!("{PREFIX}v1.2.3/termenbox-server-x86_64"));
        let Some((keys, sig)) = sign_with_ssh_keygen(&m, NAMESPACE) else {
            eprintln!("ssh-keygen not available; skipping");
            return;
        };
        assert_eq!(verify_signature(m.as_bytes(), &sig, &keys).unwrap(), "r1");
        let mut tampered = m.clone().into_bytes();
        tampered[12] ^= 1;
        assert!(verify_signature(&tampered, &sig, &keys)
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        let other = ReleaseKeys::parse(
            "r2 ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )
        .unwrap();
        assert!(verify_signature(m.as_bytes(), &sig, &other)
            .unwrap_err()
            .to_string()
            .contains("not a trusted"));
    }

    #[test]
    fn a_signature_made_for_another_purpose_is_refused() {
        let m = manifest_json("1.0.0", &format!("{PREFIX}v1.0.0/x"));
        let Some((keys, sig)) = sign_with_ssh_keygen(&m, "git") else {
            return;
        };
        assert!(verify_signature(m.as_bytes(), &sig, &keys)
            .unwrap_err()
            .to_string()
            .contains("another purpose"));
    }

    #[test]
    fn a_verified_manifest_is_parsed_and_checked() {
        let url = format!("{PREFIX}v1.2.3/termenbox-server-x86_64");
        let m = manifest_json("1.2.3", &url);
        let Some((keys, sig)) = sign_with_ssh_keygen(&m, NAMESPACE) else {
            return;
        };
        let (manifest, kid) = Manifest::verify(m.as_bytes(), &sig, &keys, PREFIX).unwrap();
        assert_eq!(kid, "r1");
        assert_eq!(manifest.binary("x86_64").unwrap().url, url);
        assert!(manifest.binary("riscv64").is_err());
        assert!(!manifest.freeze);
    }

    #[test]
    fn the_tmux_keys_are_optional_and_checked_like_the_binaries() {
        let url = format!("{PREFIX}v1.2.3/termenbox-server-x86_64-linux");
        let plain = manifest_json("1.2.3", &url);
        let h = "b".repeat(64);
        let tmux_url = format!("{PREFIX}v1.2.3/tmux-x86_64-linux");
        let with = plain.replacen(
            r#""binaries""#,
            &format!(
                r#""tmux":{{"x86_64":{{"url":"{tmux_url}","sha256":"{h}"}}}},"tmux_notices":{{"tmux-COPYING":{{"url":"{PREFIX}v1.2.3/tmux-COPYING","sha256":"{h}"}}}},"binaries""#
            ),
            1,
        );
        let Some((keys, sig)) = sign_with_ssh_keygen(&plain, NAMESPACE) else {
            return;
        };
        let (old, _) = Manifest::verify(plain.as_bytes(), &sig, &keys, PREFIX).unwrap();
        assert!(old.tmux("x86_64").is_none() && old.tmux_notices.is_empty());
        let Some((keys, sig)) = sign_with_ssh_keygen(&with, NAMESPACE) else {
            return;
        };
        let (new, _) = Manifest::verify(with.as_bytes(), &sig, &keys, PREFIX).unwrap();
        assert_eq!(new.tmux("x86_64").unwrap().url, tmux_url);
        assert!(new.tmux("aarch64").is_none());
        assert_eq!(new.tmux_notices.len(), 1);
        // a tmux entry elsewhere or with a bad hash is refused like a server binary
        let bad = with.replace(&tmux_url, "https://evil.example/tmux");
        let Some((keys, sig)) = sign_with_ssh_keygen(&bad, NAMESPACE) else {
            return;
        };
        let err = Manifest::verify(bad.as_bytes(), &sig, &keys, PREFIX)
            .unwrap_err()
            .to_string();
        assert!(err.contains("tmux is not hosted"), "{err}");
    }

    #[test]
    fn a_manifest_pointing_elsewhere_or_with_a_bad_hash_is_refused_even_when_signed() {
        for (m, why) in [
            (
                manifest_json("1.0.0", "https://evil.example/termenbox-server"),
                "not hosted",
            ),
            (
                manifest_json("1.0.0", &format!("{PREFIX}../../evil")),
                "not hosted",
            ),
            (
                manifest_json("one", &format!("{PREFIX}v1/x")),
                "not a version",
            ),
            (
                manifest_json("1.0.0", &format!("{PREFIX}v1/x")).replace(&"a".repeat(64), "xyz"),
                "not a SHA-256",
            ),
        ] {
            let Some((keys, sig)) = sign_with_ssh_keygen(&m, NAMESPACE) else {
                return;
            };
            let err = Manifest::verify(m.as_bytes(), &sig, &keys, PREFIX)
                .unwrap_err()
                .to_string();
            assert!(err.contains(why), "{err}");
        }
    }

    #[test]
    fn versions_compare_numerically() {
        let m = |v: &str| Manifest {
            version: v.into(),
            freeze: false,
            notes: String::new(),
            binaries: BTreeMap::new(),
            tmux: BTreeMap::new(),
            tmux_notices: BTreeMap::new(),
        };
        assert!(m("0.10.0").is_newer_than("0.9.9").unwrap());
        assert!(!m("1.0.0").is_newer_than("1.0.0").unwrap());
        assert!(!m("1.0.0").is_newer_than("1.0.1+abc1234").unwrap());
        assert!(m("1.0.0").is_newer_than("not-a-version").is_err());
    }

    #[test]
    fn garbage_signatures_are_refused_without_panicking() {
        let keys = ReleaseKeys::default();
        for bad in [
            "",
            "not base64 !!",
            "-----BEGIN SSH SIGNATURE-----\nAAAA\n-----END SSH SIGNATURE-----",
        ] {
            assert!(verify_signature(b"x", bad, &keys).is_err());
        }
    }

    #[test]
    fn the_built_in_key_list_parses() {
        assert!(!ReleaseKeys::parse(DEFAULT_KEYS).unwrap().is_empty());
    }
}
