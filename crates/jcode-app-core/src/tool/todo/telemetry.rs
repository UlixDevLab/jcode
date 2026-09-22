use crate::todo::{TodoGoal, TodoItem, TodoPlan};
use std::collections::HashMap;

pub(super) fn merge_confidence_history(previous: &[TodoItem], incoming: &mut [TodoItem]) {
    let prior: HashMap<&str, &TodoItem> = previous
        .iter()
        .map(|todo| (todo.id.as_str(), todo))
        .collect();
    for todo in incoming.iter_mut() {
        let previous_todo = prior.get(todo.id.as_str()).copied();
        let mut history = previous_todo
            .map(|prev| prev.confidence_history.clone())
            .unwrap_or_default();
        if history.is_empty()
            && let Some(value) = previous_todo.and_then(|prev| {
                if prev.status == "completed" {
                    prev.completion_confidence.or(prev.confidence)
                } else {
                    prev.confidence
                }
            })
        {
            history.push(value);
        }
        let observation = if todo.status == "completed" {
            todo.completion_confidence.or(todo.confidence)
        } else {
            todo.confidence
        };
        if let Some(value) = observation
            && history.last() != Some(&value)
        {
            history.push(value);
        }
        todo.confidence_history = history;
    }
}

pub(super) fn todo_telemetry_update(
    previous: &[TodoItem],
    todos: &[TodoItem],
    goals: &[TodoGoal],
    plan: &TodoPlan,
) -> crate::telemetry::TodoTelemetryUpdate {
    let previous_by_id: HashMap<&str, &TodoItem> = previous
        .iter()
        .map(|todo| (todo.id.as_str(), todo))
        .collect();
    let current_by_id: HashMap<&str, &TodoItem> =
        todos.iter().map(|todo| (todo.id.as_str(), todo)).collect();
    let todos_created = current_by_id
        .keys()
        .filter(|id| !previous_by_id.contains_key(**id))
        .count()
        .min(u32::MAX as usize) as u32;
    let todos_completed = current_by_id
        .iter()
        .filter(|(id, todo)| {
            todo.status == "completed"
                && previous_by_id
                    .get(**id)
                    .is_none_or(|previous| previous.status != "completed")
        })
        .count()
        .min(u32::MAX as usize) as u32;
    let todos_abandoned = previous_by_id
        .iter()
        .filter(|(id, todo)| todo.status != "completed" && !current_by_id.contains_key(**id))
        .count()
        .min(u32::MAX as usize) as u32;
    let current_incomplete = current_by_id
        .values()
        .filter(|todo| todo.status != "completed")
        .count()
        .min(u32::MAX as usize) as u32;
    let mut group_completion: HashMap<Option<String>, bool> = HashMap::new();
    for todo in current_by_id.values() {
        let completed = todo.status == "completed";
        group_completion
            .entry(super::goal_group_key(todo.group.as_deref()))
            .and_modify(|all_completed| *all_completed &= completed)
            .or_insert(completed);
    }
    crate::telemetry::TodoTelemetryUpdate {
        todos_created,
        todos_completed,
        todos_abandoned,
        current_incomplete,
        list_size: todos.len().min(u32::MAX as usize) as u32,
        groups_completed: group_completion
            .values()
            .filter(|completed| **completed)
            .count()
            .min(u32::MAX as usize) as u32,
        groups_total: group_completion.len().min(u32::MAX as usize) as u32,
        confidence: crate::telemetry::TelemetryScoreSummary::from_scores(
            current_by_id
                .values()
                .filter_map(|todo| todo.confidence.map(|state| state.legacy_score())),
        ),
        completion_confidence: crate::telemetry::TelemetryScoreSummary::from_scores(
            current_by_id
                .values()
                .filter_map(|todo| todo.completion_confidence.map(|state| state.legacy_score())),
        ),
        understands_user_intent: crate::telemetry::TelemetryScoreSummary::from_scores(
            plan.understands_user_intent
                .map(|state| state.legacy_score()),
        ),
        closed_feedback_loop: crate::telemetry::TelemetryScoreSummary::from_scores(
            goals
                .iter()
                .filter_map(|goal| goal.closed_feedback_loop.map(|state| state.legacy_score())),
        ),
        feedback_loop_relevance: crate::telemetry::TelemetryScoreSummary::from_scores(
            goals.iter().filter_map(|goal| {
                goal.feedback_loop_relevance
                    .map(|state| state.legacy_score())
            }),
        ),
        feedback_loop_coverage: crate::telemetry::TelemetryScoreSummary::from_scores(
            goals.iter().filter_map(|goal| {
                goal.feedback_loop_coverage
                    .map(|state| state.legacy_score())
            }),
        ),
        end_to_end_ownership: crate::telemetry::TelemetryScoreSummary::from_scores(
            goals
                .iter()
                .filter_map(|goal| goal.delivery_state.map(|state| state.legacy_score())),
        ),
    }
}

pub(super) fn record_todo_telemetry(
    previous: &[TodoItem],
    todos: &[TodoItem],
    goals: &[TodoGoal],
    plan: &TodoPlan,
) {
    crate::telemetry::record_todo_update(todo_telemetry_update(previous, todos, goals, plan));
}
