mod model;
mod store;
mod tool;

use super::{Registry, Tool};
use std::{collections::HashMap, sync::Arc};

pub(crate) use model::{BatchGrantManifest, GrantMatcher, RegisteredGrantScope, registered_scope};
pub(crate) use store::BatchGrantStore;

pub(crate) fn register_tool(
    tools: &mut HashMap<String, Arc<dyn Tool>>,
    timings: &mut Vec<(String, u128)>,
) {
    Registry::insert_tool_timed(tools, timings, "consent", tool::ConsentGrantTool::new);
}

#[cfg(test)]
mod tests;
