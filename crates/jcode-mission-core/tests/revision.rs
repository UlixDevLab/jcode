use chrono::{TimeZone, Utc};
use jcode_mission_core::{
    CanonicalArtifactHash, ConstraintKind, MissionConstraint, MissionId, MissionRevision,
    RevisionReason, RevisionSource,
};

#[test]
fn mission_revision_canonical_bytes_and_hash_match_reviewed_fixed_vector() {
    let revision = MissionRevision::new(
        MissionId::from_persisted("mission-alpha"),
        1,
        None,
        "Ship R1",
        vec![
            MissionConstraint::new(ConstraintKind::Required, "native tests"),
            MissionConstraint::new(ConstraintKind::Forbidden, "dispatch"),
        ],
        vec!["vectors pass".to_owned()],
        RevisionReason::Initial,
        RevisionSource::DirectUserPrompt {
            source_trace_id: None,
        },
        Utc.timestamp_opt(1_700_000_000, 123_456_789)
            .single()
            .unwrap(),
    )
    .unwrap();

    assert_eq!(
        hex::encode(revision.canonical_bytes()),
        "6a636f64652e72312f6d697373696f6e2d7265766973696f6e2f63616e6f6e6963616c2f76310001000000000000000d6d697373696f6e2d616c70686102000000000000000103000400000000000000075368697020523105000000000000000201000000000000000c6e61746976652074657374730200000000000000086469737061746368060000000000000001000000000000000c766563746f72732070617373070108010009000000006553f100075bcd15"
    );
    assert_eq!(
        revision.canonical_hash().as_str(),
        "sha256:v1:0f1e3fa9852631282ad90a5530619d6889dbabc9c6cd78c3055b8092319c536e"
    );
    assert!(revision.verify_canonical_hash().is_ok());
}

#[test]
fn canonical_hash_accepts_only_the_frozen_suite_and_lowercase_hex() {
    let hash: CanonicalArtifactHash =
        "sha256:v1:72b5c3dd1ec754f0ab2e53a4ad757ef40e922cfb58e239b01b12bfb50bc82c39"
            .parse()
            .unwrap();
    assert_eq!(
        hash.as_str(),
        "sha256:v1:72b5c3dd1ec754f0ab2e53a4ad757ef40e922cfb58e239b01b12bfb50bc82c39"
    );
    assert!(
        "sha256:v2:72b5c3dd1ec754f0ab2e53a4ad757ef40e922cfb58e239b01b12bfb50bc82c39"
            .parse::<CanonicalArtifactHash>()
            .is_err()
    );
    assert!(
        "sha256:v1:72B5C3DD1EC754F0AB2E53A4AD757EF40E922CFB58E239B01B12BFB50BC82C39"
            .parse::<CanonicalArtifactHash>()
            .is_err()
    );
    assert!(
        "sha256:72b5c3dd1ec754f0ab2e53a4ad757ef40e922cfb58e239b01b12bfb50bc82c39"
            .parse::<CanonicalArtifactHash>()
            .is_err()
    );
}
