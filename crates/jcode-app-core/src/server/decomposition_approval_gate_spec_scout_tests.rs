use super::*;
use crate::decomposition_materializer::{SpecScoutContradiction, SpecScoutUnstatedDefault};

#[test]
fn materialized_gate_turns_typed_scout_findings_into_approval_options() {
    let temp = tempfile::tempdir().unwrap();
    let gate_store = GateStore::at(temp.path().join("gates"));
    let scout = SpecScoutResult {
        contradictions: vec![SpecScoutContradiction {
            a: "name is the key".to_string(),
            b: "qid is the key".to_string(),
            fact: "partner conflict key".to_string(),
        }],
        unstated_defaults: vec![SpecScoutUnstatedDefault {
            behavior: "deleted rows".to_string(),
            question: "What happens to deleted rows?".to_string(),
        }],
    };
    let gate = create_for_materialized_packet(
        &packet_with_fixture_coverage(),
        &scout,
        &gate_store,
        DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
    )
    .unwrap();

    assert!(
        gate.question
            .options
            .iter()
            .any(|option| option.label == "Resolve contradiction: partner conflict key")
    );
    assert!(
        gate.question
            .options
            .iter()
            .any(|option| option.description == "What happens to deleted rows?")
    );
}
