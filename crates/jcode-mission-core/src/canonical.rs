use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

const HASH_SUITE: &str = "sha256:v1:";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct CanonicalArtifactHash(String);

impl CanonicalArtifactHash {
    pub fn from_canonical_bytes(entity: &str, bytes: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"jcode.r1/");
        hasher.update(entity.as_bytes());
        hasher.update(b"/canonical/v1\0");
        hasher.update(bytes);
        Self(format!("{HASH_SUITE}{}", hex::encode(hasher.finalize())))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalArtifactHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CanonicalArtifactHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl FromStr for CanonicalArtifactHash {
    type Err = CanonicalHashError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(encoded) = value.strip_prefix(HASH_SUITE) else {
            return if value.starts_with("sha256:") {
                Err(CanonicalHashError::UnknownSuite)
            } else {
                Err(CanonicalHashError::Malformed)
            };
        };
        if encoded.len() != 64
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            return Err(CanonicalHashError::Malformed);
        }
        hex::decode(encoded).map_err(|_| CanonicalHashError::Malformed)?;
        Ok(Self(value.to_owned()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanonicalHashError {
    Malformed,
    UnknownSuite,
}

impl fmt::Display for CanonicalHashError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed => formatter.write_str("malformed canonical artifact hash"),
            Self::UnknownSuite => formatter.write_str("unknown canonical artifact hash suite"),
        }
    }
}

impl std::error::Error for CanonicalHashError {}

#[derive(Default)]
pub(crate) struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    pub(crate) fn field(&mut self, tag: u8) {
        self.bytes.push(tag);
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn text(&mut self, value: &str) {
        self.u64(value.len() as u64);
        self.bytes.extend_from_slice(value.as_bytes());
    }

    pub(crate) fn option_text(&mut self, value: Option<&str>) {
        match value {
            None => self.u8(0),
            Some(value) => {
                self.u8(1);
                self.text(value);
            }
        }
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::CanonicalArtifactHash;

    #[test]
    fn artifact_hash_uses_the_required_prefix_and_sha256_v1_literal() {
        let hash = CanonicalArtifactHash::from_canonical_bytes("test-artifact", &[0x01, 0xa0]);
        assert_eq!(
            hash.as_str(),
            "sha256:v1:72b5c3dd1ec754f0ab2e53a4ad757ef40e922cfb58e239b01b12bfb50bc82c39"
        );
    }

    #[test]
    fn persisted_hash_rejects_unknown_suites_and_malformed_values() {
        assert!(
            serde_json::from_str::<CanonicalArtifactHash>(
                "\"sha256:v2:72b5c3dd1ec754f0ab2e53a4ad757ef40e922cfb58e239b01b12bfb50bc82c39\""
            )
            .is_err()
        );
        assert!(serde_json::from_str::<CanonicalArtifactHash>("\"sha256:v1:not-hex\"").is_err());
    }
}
