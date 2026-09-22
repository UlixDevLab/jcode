use crate::decomposition_materializer::DecompositionPacket;

pub(super) fn recommendation(packet: &DecompositionPacket) -> String {
    let coverage = &packet.coverage;
    if !coverage.snapshot_present {
        return "Coverage snapshot unavailable for this legacy materialized packet; no R/C coverage is inferred. Request a newly materialized packet before approving coverage."
            .to_string();
    }

    let waves = if coverage.waves.is_empty() {
        "- none".to_string()
    } else {
        coverage
            .waves
            .iter()
            .map(|wave| {
                let requirement_ids = if wave.requirement_ids.is_empty() {
                    "none".to_string()
                } else {
                    wave.requirement_ids.join(", ")
                };
                format!("- {}: {requirement_ids}", wave.wave_id)
            })
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let unowned = if coverage.unowned_required_ids.is_empty() {
        "none".to_string()
    } else {
        coverage.unowned_required_ids.join(", ")
    };
    format!(
        "Review the materialized requirement coverage before approving. Waves and covered IDs: {waves}. R/C IDs without an owner: {unowned}."
    )
}
