//! Typed, append-only mission revision domain primitives.
//!
//! This crate deliberately depends only on serialization, hashing, time, and
//! UUID primitives so later orchestration layers can use it without coupling
//! mission authority to session or transport state.

mod artifact;
mod canonical;
mod coverage;
mod path;
mod projection;
mod requirements;
mod revision;
mod state;
mod store;
mod transition;
mod validation;

pub use artifact::{
    EvidenceRecord, MISSION_SCHEMA_VERSION, MissionApproval, MissionArtifact, MissionStatus,
    MissionWave, ScopeEntry, WaveStatus,
};
pub use canonical::{CanonicalArtifactHash, CanonicalHashError};
pub use coverage::{MissionCoverageReport, MissionCoverageVerdict, RequirementCoverageEntry};
pub use path::{MissionPathError, MissionPaths};
pub use projection::{MissionProjection, render_projection};
pub use requirements::{ConstraintKind, MissionConstraint, MissionSuccessCriterion};
pub use revision::{
    MissionId, MissionRevision, MissionRevisionDraft, MissionRevisionError, MissionRevisionId,
    MissionRevisionRef, RevisionReason, RevisionSource,
};
pub use state::{MissionState, MissionStateError};
pub use store::{MissionStore, MissionStoreError};
pub use transition::{MissionCas, MissionTransition, MissionTransitionError};
pub use validation::MissionArtifactError;
