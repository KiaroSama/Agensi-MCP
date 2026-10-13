#[allow(dead_code)]
#[path = "../check/policy.rs"]
mod policy;

#[test]
fn test_matching_release() {
    assert!(policy::validate("2.1.0", "v2.1.0", "refs/heads/main", true).is_ok());
    let expanded = policy::expand_options(vec![
        "--manifest=path with spaces/Cargo.toml".into(),
        "--tag=v2.1.0".into(),
    ]);
    assert_eq!(
        expanded,
        vec![
            std::ffi::OsString::from("--manifest"),
            "path with spaces/Cargo.toml".into(),
            "--tag".into(),
            "v2.1.0".into()
        ]
    );
}
#[test]
fn test_artifacts_on_feature_branch() {
    assert!(policy::validate("2.1.0", "", "refs/heads/feature", true).is_ok());
}
#[test]
fn test_local_validation() {
    assert!(policy::validate("2.1.0", "v2.1.0", "", false).is_ok());
}
#[test]
fn test_different_version() {
    assert!(policy::validate("2.1.0", "v99.99.99", "refs/heads/main", true).is_err());
}
#[test]
fn test_unprefixed_tag() {
    assert!(policy::validate("2.1.0", "2.1.0", "refs/heads/main", true).is_err());
}
#[test]
fn test_shell_metacharacters() {
    assert!(policy::validate("2.1.0", "v2.1.0;echo forbidden", "refs/heads/main", true).is_err());
}
#[test]
fn test_nonmain_release() {
    assert!(policy::validate("2.1.0", "v2.1.0", "refs/heads/feature", true).is_err());
}
#[test]
fn test_prerelease_policy() {
    assert!(policy::validate("2.1.0-beta", "v2.1.0-beta", "refs/heads/main", true).is_err());
}
