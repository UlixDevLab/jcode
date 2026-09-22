//! Durable Hermes envelope correlation stored inside the authoritative gate record.
//!
//! Envelope identity is correlation-only. It carries an exact local gate binding,
//! but no transport acknowledgement or authorization semantics.

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use super::recovery::{checked_entry_name, read_entries, reject_symlink};
use super::store::{LOCK_FILE, validate_path_identifier};
use super::{GateBinding, GateRequest, GateState, GateStore};

pub const GATE_TRANSPORT_BINDING_VERSION: u32 = 1;
const ENVELOPE_ID_MAX_LEN: usize = 256;
const TRANSPARENT_PREFIX: &str = "jcode-gate-";
const DIGEST_PREFIX: &str = "jcode-gate-sha256-";
const DIGEST_DOMAIN: &[u8] = b"jcode/hermes-envelope/v1\0";

/// One versioned Hermes correlation identifier tied to exactly one local gate.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateTransportBinding {
    pub schema_version: u32,
    pub envelope_id: String,
    pub gate_id: String,
    pub binding: GateBinding,
}

impl GateTransportBinding {
    /// Build the deterministic, identifier-safe Hermes envelope correlation ID.
    pub fn for_gate(gate_id: &str, binding: GateBinding) -> Result<Self> {
        validate_identifier("gate", gate_id)?;
        binding.validate()?;
        Ok(Self {
            schema_version: GATE_TRANSPORT_BINDING_VERSION,
            envelope_id: deterministic_envelope_id(gate_id),
            gate_id: gate_id.to_string(),
            binding,
        })
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != GATE_TRANSPORT_BINDING_VERSION {
            bail!("unsupported gate transport binding version")
        }
        validate_identifier("Hermes envelope", &self.envelope_id)?;
        validate_identifier("gate", &self.gate_id)?;
        self.binding.validate()?;
        if self.envelope_id != deterministic_envelope_id(&self.gate_id) {
            bail!("gate transport envelope does not match its gate")
        }
        Ok(())
    }
}

impl GateStore {
    /// Atomically create one pending gate and its deterministic Hermes binding.
    /// Replaying the exact same durable record is idempotent.
    pub fn create_pending_transported(
        &self,
        mut gate: GateRequest,
        transport: GateTransportBinding,
    ) -> Result<GateTransportBinding> {
        if !matches!(gate.state, GateState::Pending) {
            bail!("new transported gate must begin pending")
        }
        ensure_exact_binding(&gate, &transport)?;
        if let Some(current) = &gate.transport_binding
            && current != &transport
        {
            bail!("conflicting gate transport binding")
        }
        gate.transport_binding = Some(transport.clone());
        gate.validate()?;
        let path = self.path_for(&gate.gate_id, &gate.binding.session_id)?;
        self.with_lock(|| {
            if path.exists() {
                let current = self.load_locked(&path, (&gate.gate_id, &gate.binding.session_id))?;
                if current == gate {
                    return Ok(transport);
                }
                bail!("gate already exists with conflicting transport binding")
            }
            self.save_locked(&path, &gate)?;
            Ok(transport)
        })
    }

    /// Attach a deterministic Hermes correlation binding to a legacy pending gate.
    /// Replaying the same binding is idempotent. Any different envelope, gate, or
    /// local session binding fails closed.
    pub fn attach_transport_binding(
        &self,
        transport: GateTransportBinding,
    ) -> Result<GateTransportBinding> {
        transport.validate()?;
        let path = self.path_for(&transport.gate_id, &transport.binding.session_id)?;
        self.with_lock(|| {
            let mut gate =
                self.load_locked(&path, (&transport.gate_id, &transport.binding.session_id))?;
            ensure_exact_binding(&gate, &transport)?;
            if let Some(current) = &gate.transport_binding {
                if current == &transport {
                    return Ok(transport);
                }
                bail!("conflicting gate transport binding")
            }
            if !matches!(gate.state, GateState::Pending) {
                bail!("only a pending gate may receive a transport binding")
            }
            gate.transport_binding = Some(transport.clone());
            self.save_locked(&path, &gate)?;
            Ok(transport)
        })
    }

    /// Load the durable Hermes binding without changing its gate lifecycle state.
    pub fn load_transport_binding(
        &self,
        gate_id: &str,
        session_id: &str,
    ) -> Result<Option<GateTransportBinding>> {
        let path = self.path_for(gate_id, session_id)?;
        self.with_lock(|| {
            Ok(self
                .load_locked(&path, (gate_id, session_id))?
                .transport_binding)
        })
    }

    /// List only transport-bound gates still awaiting an answer, sorted by local
    /// session ID then gate ID in ascending bytewise order.
    pub fn list_pending_transported_gates(&self) -> Result<Vec<GateRequest>> {
        self.with_lock(|| collect_pending_transported_gates(self))
    }
}

fn ensure_exact_binding(gate: &GateRequest, transport: &GateTransportBinding) -> Result<()> {
    transport.validate()?;
    if transport.gate_id != gate.gate_id || transport.binding != gate.binding {
        bail!("gate transport binding does not match its local gate binding")
    }
    Ok(())
}

fn collect_pending_transported_gates(store: &GateStore) -> Result<Vec<GateRequest>> {
    let mut gates = Vec::new();
    for session_path in read_entries(&store.root)? {
        let session_id = checked_entry_name(&session_path, "session")?;
        if session_id == LOCK_FILE {
            continue;
        }
        reject_symlink(&session_path)?;
        if !std::fs::symlink_metadata(&session_path)?
            .file_type()
            .is_dir()
        {
            bail!("unexpected entry in gate ledger root")
        }
        validate_path_identifier("session", &session_id)?;
        for record_path in read_entries(&session_path)? {
            let record_name = checked_entry_name(&record_path, "record")?;
            reject_symlink(&record_path)?;
            if !std::fs::symlink_metadata(&record_path)?
                .file_type()
                .is_file()
            {
                bail!("unexpected entry in durable gate session")
            }
            if let Some(backup_gate_id) = record_name.strip_suffix(".bak") {
                validate_path_identifier("gate", backup_gate_id)?;
                let primary = session_path.join(format!("{backup_gate_id}.json"));
                reject_symlink(&primary)?;
                if !std::fs::symlink_metadata(&primary)?.file_type().is_file() {
                    bail!("durable gate backup has no primary record")
                }
                continue;
            }
            let gate_id = record_name
                .strip_suffix(".json")
                .context("invalid durable gate record filename")?;
            validate_path_identifier("gate", gate_id)?;
            let gate = store.load_locked(&record_path, (gate_id, &session_id))?;
            if matches!(gate.state, GateState::Pending) && gate.transport_binding.is_some() {
                gates.push(gate);
            }
        }
    }
    gates.sort_by(|left, right| {
        left.binding
            .session_id
            .cmp(&right.binding.session_id)
            .then_with(|| left.gate_id.cmp(&right.gate_id))
    });
    Ok(gates)
}

fn deterministic_envelope_id(gate_id: &str) -> String {
    let transparent = format!("{TRANSPARENT_PREFIX}{gate_id}");
    if transparent.len() <= ENVELOPE_ID_MAX_LEN {
        return transparent;
    }
    let mut hasher = Sha256::new();
    hasher.update(DIGEST_DOMAIN);
    hasher.update(gate_id.as_bytes());
    format!("{DIGEST_PREFIX}{:x}", hasher.finalize())
}

fn validate_identifier(kind: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > ENVELOPE_ID_MAX_LEN
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        bail!("invalid {kind} identifier")
    }
    Ok(())
}
