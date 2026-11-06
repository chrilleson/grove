use grove::setup::RepoConfig;

#[test]
fn user_file_wins_key_by_key_over_committed_file() {
    let committed = r#"
        base = "origin/develop"
        [setup]
        copy = [".env"]
        run = ["npm ci"]
        [ports]
        PORT = 0
    "#;
    let user = r#"
        [setup]
        run = ["pnpm install"]
        [ports]
        API_PORT = 1
    "#;
    let c = RepoConfig::merged(Some(committed), Some(user)).unwrap();
    assert_eq!(c.base.as_deref(), Some("origin/develop"));
    assert_eq!(c.setup.copy, [".env"]);
    assert_eq!(c.setup.run, ["pnpm install"]);
    assert_eq!(c.ports.get("PORT"), Some(&0));
    assert_eq!(c.ports.get("API_PORT"), Some(&1));
}

#[test]
fn no_files_means_empty_setup() {
    let c = RepoConfig::merged(None, None).unwrap();
    assert!(c.setup.run.is_empty() && c.ports.is_empty() && c.base.is_none());
}
