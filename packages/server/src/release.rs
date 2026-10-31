//! Signed release manifests (decision 0076, guardrail updates-are-verified-and-reversible). CI signs
//! `manifest.json` with an Ed25519 release key in the OpenSSH signature format, so a host can check it with
//! plain `ssh-keygen -Y verify` before it trusts anything it downloaded (the installer), and the server can check
//! the same signature with the key embedded in this binary (the updater). The download host is never trusted.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};
use semver::{BuildMetadata, Version};
use serde::Deserialize;
use sha2::{Digest, Sha256, Sha512};

/// The `ssh-keygen -Y sign -n` namespace; a signature made for anything else is refused.
pub const NAMESPACE: &str = "termenbox-release";
/// Release assets are only fetched from this project's GitHub releases.
pub const URL_PREFIX: &str = "https://github.com/termenbox/termenbox/releases/download/";
const DEFAULT_KEYS: &str = include_str!("../release-keys.txt");

/// A release stream. A key signs one channel, and a binary trusts the channels in `trusts`.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    fn parse(s: &str) -> Result<Self> {
        match s {
            "stable" => Ok(Self::Stable),
            "beta" => Ok(Self::Beta),
            "nightly" => Ok(Self::Nightly),
            _ => bail!("`{s}` is not a channel: stable, beta or nightly"),
        }
    }

    /// The channel a running version belongs to, from its prerelease label: `nightly.*` is nightly, `beta.*` and
    /// `rc.*` are beta, anything else (including no prerelease) is stable.
    pub fn of_version(version: &str) -> Self {
        let pre = Version::parse(version).map(|v| v.pre.to_string());
        match pre.as_deref().unwrap_or("").split('.').next().unwrap_or("") {
            "nightly" => Self::Nightly,
            "beta" | "rc" => Self::Beta,
            _ => Self::Stable,
        }
    }

    /// Whether a binary of this channel runs a manifest of `other`: a beta binary follows the stable line too (the
    /// beta alias can point at a stable release); stable and nightly follow only themselves.
    pub fn trusts(self, other: Channel) -> bool {
        self == other || (self == Self::Beta && other == Self::Stable)
    }
}

/// A validly signed manifest of a channel this binary does not follow. The updater treats it as "nothing for me
/// here" (a stable server pointed at a beta alias), not as a broken release.
#[derive(Debug)]
pub struct ChannelNotFollowed(pub Channel);

impl std::fmt::Display for ChannelNotFollowed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "this binary does not follow the {:?} channel", self.0)
    }
}

impl std::error::Error for ChannelNotFollowed {}

/// The trusted release public keys: `kid ssh-ed25519 <base64 blob> [channel=<stable|beta|nightly>]` per line. A line
/// without a channel is the stable key, so a list written before channels existed keeps its meaning.
#[derive(Clone, Default)]
pub struct ReleaseKeys(Vec<(String, [u8; 32], Channel)>);

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
                bail!("bad release key line: expected `kid ssh-ed25519 <key> [channel=...]`");
            };
            let channel = match (parts.next(), parts.next()) {
                (None, _) => Channel::Stable,
                (Some(f), None) if f.starts_with("channel=") => {
                    Channel::parse(&f["channel=".len()..])?
                }
                _ => bail!("bad release key line: the only extra field is `channel=...`"),
            };
            keys.push((
                kid.to_string(),
                ed25519_from_blob(&STANDARD.decode(blob)?)?,
                channel,
            ));
        }
        Ok(Self(keys))
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn find(&self, key: &[u8; 32]) -> Option<(&str, Channel)> {
        self.0
            .iter()
            .find(|(_, k, _)| k == key)
            .map(|(kid, _, c)| (kid.as_str(), *c))
    }

    /// Whether key `kid` may sign a manifest for `channel`: a nightly key cannot sign a stable release.
    pub fn allows_channel(&self, kid: &str, channel: Channel) -> bool {
        self.0.iter().any(|(k, _, c)| k == kid && *c == channel)
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

    let (kid, _) = keys
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
    /// Full SemVer: `0.1.36`, `0.1.36-beta.2`, `0.1.36-nightly.20261022`.
    pub version: String,
    /// The stream this release belongs to; the signing key must be bound to it. A manifest from before channels
    /// existed has none and is stable, which only a stable key may sign, so the default grants nothing.
    #[serde(default)]
    pub channel: Channel,
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
    /// The container image of this release by architecture, as `sha256:` digests (decision 0154). Absent in a release
    /// that is not a container release, and in releases from before it existed.
    #[serde(default)]
    pub image: BTreeMap<String, String>,
}

impl Manifest {
    /// Verifies the signature first, then parses and validates; nothing in an unverified manifest is used.
    /// `running` is the channel of the binary asking: it refuses a manifest of a channel it does not follow, and a
    /// manifest signed by a key that is not bound to the manifest's channel.
    pub fn verify(
        bytes: &[u8],
        signature: &str,
        keys: &ReleaseKeys,
        url_prefix: &str,
        running: Channel,
    ) -> Result<(Manifest, String)> {
        let kid = verify_signature(bytes, signature, keys)?;
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|e| anyhow!("bad manifest: {e}"))?;
        parse_version(&manifest.version)?;
        if !keys.allows_channel(&kid, manifest.channel) {
            bail!("key {kid} may not sign a {:?} release", manifest.channel);
        }
        if !running.trusts(manifest.channel) {
            return Err(ChannelNotFollowed(manifest.channel).into());
        }
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
        for (arch, digest) in &manifest.image {
            if !matches!(arch.as_str(), "x86_64" | "aarch64") || !is_image_digest(digest) {
                bail!("the {arch} image is not a sha256 digest");
            }
        }
        Ok((manifest, kid))
    }

    /// The image digest for `arch`, when this release is a container release.
    pub fn image(&self, arch: &str) -> Option<&str> {
        self.image.get(arch).map(String::as_str)
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

    /// Whether this release has a higher major version than `current` (a breaking change an owner must confirm).
    pub fn is_major_bump_from(&self, current: &str) -> Result<bool> {
        Ok(parse_version(&self.version)?.major > parse_version(current)?.major)
    }

    /// Whether this release is newer than the running `current` version.
    pub fn is_newer_than(&self, current: &str) -> Result<bool> {
        Ok(parse_version(&self.version)? > parse_version(current)?)
    }
}

/// `sha256:` and 64 lowercase hex digits: the only form that is passed to `docker pull` as `repo@digest`.
fn is_image_digest(d: &str) -> bool {
    d.strip_prefix("sha256:")
        .is_some_and(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// SemVer with build metadata dropped (`0.1.36+abc1234` is the same release as `0.1.36`), so a prerelease sorts
/// below its release and `nightly.20261022 < nightly.20261023`.
fn parse_version(v: &str) -> Result<Version> {
    let mut version = Version::parse(v)
        .map_err(|_| anyhow!("`{v}` is not a version like 1.2.3 or 1.2.3-beta.1"))?;
    version.build = BuildMetadata::EMPTY;
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const PREFIX: &str = URL_PREFIX;

    fn manifest_json(version: &str, url: &str) -> String {
        format!(
            r#"{{"version":"{version}","channel":"stable","freeze":false,"notes":"","binaries":{{"x86_64":{{"url":"{url}","sha256":"{}"}}}}}}"#,
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
        let (manifest, kid) =
            Manifest::verify(m.as_bytes(), &sig, &keys, PREFIX, Channel::Stable).unwrap();
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
        let (old, _) =
            Manifest::verify(plain.as_bytes(), &sig, &keys, PREFIX, Channel::Stable).unwrap();
        assert!(old.tmux("x86_64").is_none() && old.tmux_notices.is_empty());
        let Some((keys, sig)) = sign_with_ssh_keygen(&with, NAMESPACE) else {
            return;
        };
        let (new, _) =
            Manifest::verify(with.as_bytes(), &sig, &keys, PREFIX, Channel::Stable).unwrap();
        assert_eq!(new.tmux("x86_64").unwrap().url, tmux_url);
        assert!(new.tmux("aarch64").is_none());
        assert_eq!(new.tmux_notices.len(), 1);
        // a tmux entry elsewhere or with a bad hash is refused like a server binary
        let bad = with.replace(&tmux_url, "https://evil.example/tmux");
        let Some((keys, sig)) = sign_with_ssh_keygen(&bad, NAMESPACE) else {
            return;
        };
        let err = Manifest::verify(bad.as_bytes(), &sig, &keys, PREFIX, Channel::Stable)
            .unwrap_err()
            .to_string();
        assert!(err.contains("tmux is not hosted"), "{err}");
    }

    #[test]
    fn the_image_key_is_optional_and_only_sha256_digests_for_known_architectures_pass() {
        let url = format!("{PREFIX}v1.2.3/termenbox-server-x86_64-linux");
        let plain = manifest_json("1.2.3", &url);
        let digest = format!("sha256:{}", "c".repeat(64));
        let with = |image: &str| {
            plain.replacen(
                r#""binaries""#,
                &format!(r#""image":{image},"binaries""#),
                1,
            )
        };
        let verify = |m: &str| {
            let (keys, sig) = sign_with_ssh_keygen(m, NAMESPACE)?;
            Some(Manifest::verify(
                m.as_bytes(),
                &sig,
                &keys,
                PREFIX,
                Channel::Stable,
            ))
        };
        let Some(old) = verify(&plain) else { return };
        assert!(old.unwrap().0.image("x86_64").is_none());
        let good = verify(&with(&format!(r#"{{"x86_64":"{digest}"}}"#)))
            .unwrap()
            .unwrap()
            .0;
        assert_eq!(good.image("x86_64"), Some(digest.as_str()));
        assert!(good.image("aarch64").is_none());
        // a tag, an uppercase or short digest, a flag-looking value and an unknown architecture are all refused
        for bad in [
            r#"{"x86_64":"latest"}"#.to_string(),
            format!(r#"{{"x86_64":"sha256:{}"}}"#, "C".repeat(64)),
            r#"{"x86_64":"sha256:abc"}"#.to_string(),
            r#"{"x86_64":"--privileged"}"#.to_string(),
            format!(r#"{{"riscv64":"{digest}"}}"#),
        ] {
            let err = verify(&with(&bad)).unwrap().unwrap_err().to_string();
            assert!(err.contains("image is not a sha256 digest"), "{bad}: {err}");
        }
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
            let err = Manifest::verify(m.as_bytes(), &sig, &keys, PREFIX, Channel::Stable)
                .unwrap_err()
                .to_string();
            assert!(err.contains(why), "{err}");
        }
    }

    #[test]
    fn versions_compare_numerically() {
        let m = |v: &str| Manifest {
            version: v.into(),
            channel: Channel::Stable,
            freeze: false,
            notes: String::new(),
            binaries: BTreeMap::new(),
            tmux: BTreeMap::new(),
            tmux_notices: BTreeMap::new(),
            image: BTreeMap::new(),
        };
        assert!(m("0.10.0").is_newer_than("0.9.9").unwrap());
        assert!(!m("1.0.0").is_newer_than("1.0.0").unwrap());
        assert!(!m("1.0.0").is_newer_than("1.0.1+abc1234").unwrap());
        assert!(m("1.0.0").is_newer_than("not-a-version").is_err());
    }

    #[test]
    fn prereleases_order_by_semver() {
        let m = |v: &str| Manifest {
            version: v.into(),
            channel: Channel::Nightly,
            freeze: false,
            notes: String::new(),
            binaries: BTreeMap::new(),
            tmux: BTreeMap::new(),
            tmux_notices: BTreeMap::new(),
            image: BTreeMap::new(),
        };
        let (a, b, rel) = (
            "0.1.36-nightly.20261022",
            "0.1.36-nightly.20261023",
            "0.1.37",
        );
        assert!(m(b).is_newer_than(a).unwrap() && !m(a).is_newer_than(b).unwrap());
        assert!(m(rel).is_newer_than(b).unwrap());
        // a nightly of 0.1.36 is below the 0.1.36 release, and build metadata never counts
        assert!(m("0.1.36").is_newer_than(b).unwrap());
        assert!(!m(b).is_newer_than("0.1.36").unwrap());
        assert!(!m(a)
            .is_newer_than("0.1.36-nightly.20261022+abc1234")
            .unwrap());
        assert!(m("0.1.36-beta.2").is_newer_than("0.1.36-beta.1").unwrap());
        assert!(m("0.1.36-rc.1").is_newer_than("0.1.36-beta.9").unwrap());
    }

    #[test]
    fn a_running_version_names_its_channel() {
        assert_eq!(Channel::of_version("0.1.36"), Channel::Stable);
        assert_eq!(Channel::of_version("0.1.36+abc1234"), Channel::Stable);
        assert_eq!(Channel::of_version("0.1.36-beta.2"), Channel::Beta);
        assert_eq!(Channel::of_version("0.1.36-rc.1+abc1234"), Channel::Beta);
        assert_eq!(
            Channel::of_version("0.1.36-nightly.20261022"),
            Channel::Nightly
        );
        assert_eq!(Channel::of_version("garbage"), Channel::Stable);
    }

    #[test]
    fn trust_sets_follow_the_channels() {
        use Channel::*;
        assert!(Stable.trusts(Stable) && !Stable.trusts(Beta) && !Stable.trusts(Nightly));
        assert!(Beta.trusts(Beta) && Beta.trusts(Stable) && !Beta.trusts(Nightly));
        assert!(Nightly.trusts(Nightly) && !Nightly.trusts(Stable) && !Nightly.trusts(Beta));
    }

    const KEY: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIDNH6+yZgMpZl+tW8C/D+9Ga0bN39vXzIIJMKmHB6auf";

    #[test]
    fn key_lines_bind_a_key_to_a_channel_and_old_lines_are_stable() {
        let keys = ReleaseKeys::parse(&format!(
            "r1 {KEY}\nb1 {KEY} channel=beta\nn1 {KEY} channel=nightly"
        ))
        .unwrap();
        assert!(keys.allows_channel("r1", Channel::Stable));
        assert!(
            keys.allows_channel("b1", Channel::Beta) && !keys.allows_channel("b1", Channel::Stable)
        );
        assert!(
            keys.allows_channel("n1", Channel::Nightly)
                && !keys.allows_channel("n1", Channel::Stable)
        );
        assert!(!keys.allows_channel("missing", Channel::Stable));
        for bad in [
            format!("x {KEY} channel=gold"),
            format!("x {KEY} beta"),
            format!("x {KEY} channel=beta extra"),
        ] {
            assert!(ReleaseKeys::parse(&bad).is_err(), "{bad}");
        }
    }

    /// Signs `manifest` with a throwaway key listed under `kid` for `channel`.
    fn signed_by(manifest: &str, kid: &str, channel: &str) -> Option<(ReleaseKeys, String)> {
        let (keys, sig) = sign_with_ssh_keygen(manifest, NAMESPACE)?;
        let line = keys.0[0].1;
        let mut blob = Vec::new();
        put(&mut blob, b"ssh-ed25519");
        put(&mut blob, &line);
        let text = format!(
            "{kid} ssh-ed25519 {} channel={channel}",
            STANDARD.encode(blob)
        );
        Some((ReleaseKeys::parse(&text).ok()?, sig))
    }

    #[test]
    fn a_key_signs_only_its_own_channel_and_a_binary_follows_only_its_trust_set() {
        let url = format!("{PREFIX}v0.1.36-nightly.20261022/termenbox-server-x86_64-linux");
        let nightly = manifest_json("0.1.36-nightly.20261022", &url)
            .replace(r#""channel":"stable""#, r#""channel":"nightly""#);
        let stable_claim = manifest_json("0.1.36", &url);
        // the nightly key signing a manifest that claims stable is refused
        let Some((keys, sig)) = signed_by(&stable_claim, "n1", "nightly") else {
            return;
        };
        let err = Manifest::verify(
            stable_claim.as_bytes(),
            &sig,
            &keys,
            PREFIX,
            Channel::Stable,
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("may not sign"), "{err}");
        // the nightly key signing nightly passes for a nightly binary only
        let Some((keys, sig)) = signed_by(&nightly, "n1", "nightly") else {
            return;
        };
        let ok = |running| Manifest::verify(nightly.as_bytes(), &sig, &keys, PREFIX, running);
        assert_eq!(ok(Channel::Nightly).unwrap().1, "n1");
        assert!(ok(Channel::Stable)
            .unwrap_err()
            .to_string()
            .contains("does not follow"));
        assert!(ok(Channel::Beta).is_err());
        // a stable manifest reaches stable and beta binaries, not nightly ones
        let Some((keys, sig)) = signed_by(&stable_claim, "r1", "stable") else {
            return;
        };
        let ok = |running| Manifest::verify(stable_claim.as_bytes(), &sig, &keys, PREFIX, running);
        assert!(ok(Channel::Stable).is_ok() && ok(Channel::Beta).is_ok());
        assert!(ok(Channel::Nightly).is_err());
    }

    #[test]
    fn a_manifest_without_a_channel_is_stable_and_only_a_stable_key_may_sign_it() {
        let m =
            manifest_json("1.0.0", &format!("{PREFIX}v1/x")).replace(r#""channel":"stable","#, "");
        let Some((keys, sig)) = sign_with_ssh_keygen(&m, NAMESPACE) else {
            return;
        };
        let (manifest, _) =
            Manifest::verify(m.as_bytes(), &sig, &keys, PREFIX, Channel::Stable).unwrap();
        assert_eq!(manifest.channel, Channel::Stable);
        let Some((keys, sig)) = signed_by(&m, "n1", "nightly") else {
            return;
        };
        let err = Manifest::verify(m.as_bytes(), &sig, &keys, PREFIX, Channel::Nightly)
            .unwrap_err()
            .to_string();
        assert!(err.contains("may not sign"), "{err}");
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
