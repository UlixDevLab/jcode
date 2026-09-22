use super::*;

/// Point `JCODE_HOME` at a temp dir so global-role discovery is hermetic.
fn with_global_home<T>(f: impl FnOnce(&Path) -> T) -> T {
    let _guard = crate::storage::lock_test_env();
    let prev = std::env::var_os("JCODE_HOME");
    let temp = tempfile::TempDir::new().unwrap();
    crate::env::set_var("JCODE_HOME", temp.path());

    let out = f(temp.path());

    match prev {
        Some(prev) => crate::env::set_var("JCODE_HOME", prev),
        None => crate::env::remove_var("JCODE_HOME"),
    }
    out
}

fn write_role(dir: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(format!("{name}.md")), body).unwrap();
}

#[test]
fn summary_prefers_first_prose_line_over_heading() {
    let body = "# DevOps\n\nOwn deployment and live infrastructure.\n";
    assert_eq!(summarize(body), "Own deployment and live infrastructure.");
}

#[test]
fn summary_falls_back_to_heading_when_body_is_only_a_title() {
    assert_eq!(summarize("# Frontend\n"), "Frontend");
}

#[test]
fn summary_skips_front_matter_but_not_later_horizontal_rules() {
    let body = "---\nname: verify\n---\n\nIndependent acceptance gate.\n";
    assert_eq!(summarize(body), "Independent acceptance gate.");
}

#[test]
fn summary_is_truncated_on_a_character_boundary() {
    // Multi-byte characters would panic a naive byte slice.
    let body = "ы".repeat(500);
    let summary = summarize(&body);
    assert!(summary.chars().count() <= MAX_SUMMARY_CHARS);
    assert!(summary.ends_with('…'));
}

#[test]
fn discover_returns_nothing_when_no_roles_exist() {
    with_global_home(|_| {
        let project = tempfile::TempDir::new().unwrap();
        assert!(discover(Some(project.path())).is_empty());
        assert!(build_roles_prompt(&[]).is_none());
    });
}

#[test]
fn discover_finds_global_roles_sorted_by_name() {
    with_global_home(|home| {
        let roles_dir = home.join("roles");
        write_role(&roles_dir, "verify", "Read-only acceptance gate.\n");
        write_role(&roles_dir, "devops", "Own live infrastructure.\n");

        let roles = discover(None);
        let names: Vec<&str> = roles.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["devops", "verify"]);
        assert_eq!(roles[0].summary, "Own live infrastructure.");
        assert!(!roles[0].overrides_global);
    });
}

#[test]
fn project_role_overrides_global_role_of_the_same_name() {
    with_global_home(|home| {
        write_role(&home.join("roles"), "frontend", "Global frontend rules.\n");

        let project = tempfile::TempDir::new().unwrap();
        write_role(
            &project_roles_dir(project.path()),
            "frontend",
            "This repo ships Vue, not React.\n",
        );

        let roles = discover(Some(project.path()));
        assert_eq!(roles.len(), 1, "override must not duplicate the role");
        assert_eq!(roles[0].summary, "This repo ships Vue, not React.");
        assert!(roles[0].overrides_global);
        assert!(roles[0].path.starts_with(project.path()));
    });
}

#[test]
fn project_roles_are_added_alongside_global_roles() {
    with_global_home(|home| {
        write_role(&home.join("roles"), "devops", "Global devops.\n");

        let project = tempfile::TempDir::new().unwrap();
        write_role(
            &project_roles_dir(project.path()),
            "trading-lab",
            "Run the experiment loop.\n",
        );

        let roles = discover(Some(project.path()));
        let names: Vec<&str> = roles.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["devops", "trading-lab"]);
    });
}

#[test]
fn discovery_ignores_non_markdown_files() {
    with_global_home(|home| {
        let roles_dir = home.join("roles");
        write_role(&roles_dir, "devops", "Own live infrastructure.\n");
        std::fs::write(roles_dir.join("notes.txt"), "not a role").unwrap();
        std::fs::create_dir_all(roles_dir.join("nested")).unwrap();

        let roles = discover(None);
        assert_eq!(roles.len(), 1);
        assert_eq!(roles[0].name, "devops");
    });
}

#[test]
fn two_projects_do_not_see_each_others_roles() {
    // The whole point of resolving against the session working dir: a shared
    // daemon serving many projects must not leak one project's roles.
    with_global_home(|_| {
        let a = tempfile::TempDir::new().unwrap();
        let b = tempfile::TempDir::new().unwrap();
        write_role(&project_roles_dir(a.path()), "alpha", "Only in A.\n");
        write_role(&project_roles_dir(b.path()), "beta", "Only in B.\n");

        let a_names: Vec<String> = discover(Some(a.path()))
            .into_iter()
            .map(|r| r.name)
            .collect();
        let b_names: Vec<String> = discover(Some(b.path()))
            .into_iter()
            .map(|r| r.name)
            .collect();
        assert_eq!(a_names, vec!["alpha".to_string()]);
        assert_eq!(b_names, vec!["beta".to_string()]);
    });
}

#[test]
fn prompt_lists_names_paths_and_summaries_but_not_bodies() {
    with_global_home(|home| {
        write_role(
            &home.join("roles"),
            "devops",
            "Own live infrastructure.\n\nSECRET_BODY_MARKER should never be inlined.\n",
        );

        let roles = discover(None);
        let prompt = build_roles_prompt(&roles).expect("roles exist");
        assert!(prompt.contains("`devops`"));
        assert!(prompt.contains("Own live infrastructure."));
        assert!(prompt.contains("devops.md"));
        assert!(
            !prompt.contains("SECRET_BODY_MARKER"),
            "role bodies must stay lazy: {prompt}"
        );
    });
}

#[test]
fn prompt_marks_project_overrides() {
    with_global_home(|home| {
        write_role(&home.join("roles"), "frontend", "Global.\n");
        let project = tempfile::TempDir::new().unwrap();
        write_role(&project_roles_dir(project.path()), "frontend", "Local.\n");

        let prompt = build_roles_prompt(&discover(Some(project.path()))).unwrap();
        assert!(prompt.contains("project override"), "{prompt}");
    });
}

#[test]
fn role_count_is_bounded() {
    with_global_home(|home| {
        let roles_dir = home.join("roles");
        for i in 0..(MAX_ROLES_PER_DIR + 20) {
            write_role(&roles_dir, &format!("role{i:03}"), "Body.\n");
        }
        assert_eq!(discover(None).len(), MAX_ROLES_PER_DIR);
    });
}

/// The role catalog lives in the *static* system prompt, so changing it
/// legitimately invalidates a warm KV cache prefix. Config and skill reloads
/// already document their invalidations; roles must too, or the TUI reports an
/// unexplained "harness: system changed" alarm and the operator has no way to
/// tell a real harness bug from their own edit.
#[test]
fn changing_the_role_catalog_documents_the_cache_invalidation() {
    with_global_home(|home| {
        let roles_dir = home.join("roles");
        write_role(&roles_dir, "devops", "Own live infrastructure.\n");

        // First render establishes the baseline: nothing changed yet.
        let start = std::time::Instant::now();
        build_roles_prompt(&discover(None));
        assert!(
            crate::cache_invalidation::most_recent_since(start).is_none(),
            "a first render must not claim the catalog changed"
        );

        // Re-rendering an unchanged catalog must stay silent, otherwise every
        // turn would report a phantom invalidation.
        let before_stable = std::time::Instant::now();
        build_roles_prompt(&discover(None));
        assert!(
            crate::cache_invalidation::most_recent_since(before_stable).is_none(),
            "an unchanged catalog must not be reported as a change"
        );

        // Adding a role changes the prompt and must be documented.
        write_role(&roles_dir, "lab", "Run the experiment loop.\n");
        let before_change = std::time::Instant::now();
        build_roles_prompt(&discover(None));
        let recorded = crate::cache_invalidation::most_recent_since(before_change)
            .expect("adding a role must document the invalidation");
        assert_eq!(recorded.source, "role change");
    });
}

#[test]
fn editing_a_role_summary_documents_the_invalidation_but_editing_its_body_does_not() {
    with_global_home(|home| {
        let roles_dir = home.join("roles");
        write_role(
            &roles_dir,
            "devops",
            "Own live infrastructure.\n\nBody one.\n",
        );
        build_roles_prompt(&discover(None));

        // Only the summary reaches the prompt, so a body edit changes nothing
        // the model sees and must not be reported.
        write_role(
            &roles_dir,
            "devops",
            "Own live infrastructure.\n\nBody two, longer.\n",
        );
        let before_body = std::time::Instant::now();
        build_roles_prompt(&discover(None));
        assert!(
            crate::cache_invalidation::most_recent_since(before_body).is_none(),
            "a body-only edit does not change the prompt and must not be reported"
        );

        // The summary does reach the prompt.
        write_role(
            &roles_dir,
            "devops",
            "Now owns deploys too.\n\nBody two, longer.\n",
        );
        let before_summary = std::time::Instant::now();
        build_roles_prompt(&discover(None));
        assert!(
            crate::cache_invalidation::most_recent_since(before_summary).is_some(),
            "a summary edit changes the prompt and must be documented"
        );
    });
}
