use super::{DEFAULT_NAMESPACE, DependencyRef, parse_dependency, validate_namespace_name};

#[test]
fn parses_bare_service_as_default_namespace() {
    let dep = parse_dependency("redis").expect("parse");
    assert_eq!(
        dep,
        DependencyRef {
            namespace: DEFAULT_NAMESPACE.into(),
            service: "redis".into(),
            explicit_namespace: false,
        }
    );
}

#[test]
fn parses_empty_namespace_prefix_as_default() {
    let dep = parse_dependency(":redis").expect("parse");
    assert_eq!(
        dep,
        DependencyRef {
            namespace: DEFAULT_NAMESPACE.into(),
            service: "redis".into(),
            explicit_namespace: false,
        }
    );
}

#[test]
fn parses_explicit_namespace() {
    let dep = parse_dependency("staging:redis").expect("parse");
    assert_eq!(
        dep,
        DependencyRef {
            namespace: "staging".into(),
            service: "redis".into(),
            explicit_namespace: true,
        }
    );
}

#[test]
fn rejects_two_colons() {
    let err = parse_dependency("a:b:c").unwrap_err().to_string();
    assert!(err.contains("a:b:c"), "error mentions raw input: {err}");
}

#[test]
fn rejects_empty_service_after_colon() {
    let err = parse_dependency("staging:").unwrap_err().to_string();
    assert!(err.contains("service"), "error mentions service: {err}");
}

#[test]
fn rejects_empty_input() {
    let err = parse_dependency("").unwrap_err().to_string();
    assert!(err.contains("non-empty"), "error mentions non-empty: {err}");
}

#[test]
fn rejects_namespace_with_uppercase() {
    let err = parse_dependency("Staging:redis").unwrap_err().to_string();
    assert!(
        err.contains("Staging:redis"),
        "error mentions raw input: {err}"
    );
}

#[test]
fn rejects_namespace_starting_with_dash() {
    let err = parse_dependency("-bad:redis").unwrap_err().to_string();
    assert!(
        err.contains("-bad:redis"),
        "error mentions raw input: {err}"
    );
}

#[test]
fn validate_namespace_name_accepts_default() {
    validate_namespace_name(DEFAULT_NAMESPACE).expect("accept default");
}

#[test]
fn validate_namespace_name_accepts_alnum_dash_underscore() {
    validate_namespace_name("staging-1").unwrap();
    validate_namespace_name("ns_2").unwrap();
    validate_namespace_name("0abc").unwrap();
}

#[test]
fn validate_namespace_name_rejects_too_long() {
    let too_long = "a".repeat(65);
    validate_namespace_name(&too_long).unwrap_err();
}

#[test]
fn validate_namespace_name_rejects_empty() {
    validate_namespace_name("").unwrap_err();
}
