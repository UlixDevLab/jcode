//! Immutable persistence for mandatory spec-scout results.

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct SpecScoutReceipt {
    packet_hash: String,
    result: SpecScoutResult,
}

impl PacketStore {
    fn spec_scout_path(&self, session_id: &str, revision: u64) -> PathBuf {
        self.root.join(format!(
            "{}-{}.spec-scout.json",
            sanitize(session_id),
            revision
        ))
    }

    /// Return the durable, hash-bound result of the mandatory scout if present.
    /// Any malformed or nearby-packet receipt is an error, never an empty result.
    pub(crate) fn load_spec_scout(
        &self,
        packet: &DecompositionPacket,
    ) -> Result<Option<SpecScoutResult>, MaterializerError> {
        let path = self.spec_scout_path(&packet.session_id, packet.revision);
        if !path.exists() {
            return Ok(None);
        }
        let receipt: SpecScoutReceipt = read_json(&path)?;
        if receipt.packet_hash != packet.canonical_hash {
            return Err(MaterializerError::PacketConflict {
                revision: packet.revision,
            });
        }
        receipt
            .result
            .validate()
            .map_err(MaterializerError::InvalidSpecScoutResult)?;
        Ok(Some(receipt.result))
    }

    /// Persist the first valid result for this immutable packet. Replays must
    /// produce the same result instead of asking a second scout and drifting the
    /// already-bound approval choices.
    pub(crate) fn record_spec_scout(
        &self,
        packet: &DecompositionPacket,
        result: &SpecScoutResult,
    ) -> Result<(), MaterializerError> {
        result
            .validate()
            .map_err(MaterializerError::InvalidSpecScoutResult)?;
        fs::create_dir_all(&self.root)?;
        let path = self.spec_scout_path(&packet.session_id, packet.revision);
        let receipt = SpecScoutReceipt {
            packet_hash: packet.canonical_hash.clone(),
            result: result.clone(),
        };
        if path.exists() {
            let existing: SpecScoutReceipt = read_json(&path)?;
            if existing != receipt {
                return Err(MaterializerError::PacketConflict {
                    revision: packet.revision,
                });
            }
            return Ok(());
        }
        write_json(&path, &receipt)
    }
}
