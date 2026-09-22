//! Typed findings required before a materialized decomposition can be approved.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpecScoutResult {
    pub contradictions: Vec<SpecScoutContradiction>,
    pub unstated_defaults: Vec<SpecScoutUnstatedDefault>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpecScoutContradiction {
    pub a: String,
    pub b: String,
    pub fact: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SpecScoutUnstatedDefault {
    pub behavior: String,
    pub question: String,
}

impl SpecScoutResult {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for contradiction in &self.contradictions {
            if contradiction.a.trim().is_empty()
                || contradiction.b.trim().is_empty()
                || contradiction.fact.trim().is_empty()
            {
                return Err("spec scout contradiction findings require a, b, and fact".to_string());
            }
        }
        for default in &self.unstated_defaults {
            if default.behavior.trim().is_empty() || default.question.trim().is_empty() {
                return Err(
                    "spec scout unstated-default findings require behavior and question"
                        .to_string(),
                );
            }
        }
        Ok(())
    }
}
