use clap::Parser;

#[derive(clap::Parser, Debug)]
struct Wrapper {
    #[command(flatten)]
    args: super::CheckArgs,
}

#[test]
fn check_parses_namespace_flag() {
    let parsed = Wrapper::parse_from(["test", "--namespace", "staging", "web"]);
    assert_eq!(parsed.args.pipeline.namespace.as_deref(), Some("staging"));
    assert_eq!(
        parsed.args.pipeline.apps.as_deref(),
        Some(&["web".to_string()][..])
    );
}

#[test]
fn check_namespace_absent_yields_none() {
    let parsed = Wrapper::parse_from(["test", "web"]);
    assert!(parsed.args.pipeline.namespace.is_none());
}
