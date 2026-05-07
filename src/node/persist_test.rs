use super::*;
use crate::tui::state::{NodeFormMode, NodeFormState};

#[test]
fn parse_node_form_trims_and_parses_port() {
    let mut form = NodeFormState::blank(NodeFormMode::Add);
    form.name = "  edge-1  ".into();
    form.ip = " 10.0.0.1 ".into();
    form.port = "2222".into();
    form.user = "root".into();
    form.key_path = "  /tmp/key  ".into();
    let input = parse_node_form(form).unwrap();
    assert_eq!(input.name, "edge-1");
    assert_eq!(input.ip, "10.0.0.1");
    assert_eq!(input.port, 2222);
    assert_eq!(input.key_path.as_deref(), Some("/tmp/key"));
}

#[test]
fn parse_node_form_rejects_bad_port() {
    let mut form = NodeFormState::blank(NodeFormMode::Add);
    form.name = "n".into();
    form.ip = "1.1.1.1".into();
    form.port = "abc".into();
    form.user = "u".into();
    let err = parse_node_form(form).unwrap_err();
    assert!(format!("{err}").contains("invalid port"));
}
