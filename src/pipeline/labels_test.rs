use super::{ComposeRewriteOutcome, is_docker_compose_file, maybe_inject_compose_labels};
use crate::node::types::NodeRecord;
use std::path::PathBuf;

fn run(content: &str) -> ComposeRewriteOutcome {
    let path = PathBuf::from("docker-compose.yml");
    let template_values = serde_json::json!({
        "service": "web",
        "namespace": "default",
        "app": { "name": "demo" },
    });
    let node = NodeRecord::Local();
    maybe_inject_compose_labels(&path, content, &template_values, &node).unwrap()
}

#[test]
fn detects_long_form_build_block() {
    let yaml = "\
services:
  web:
    build:
      context: .
      dockerfile: Dockerfile
";
    assert!(run(yaml).has_build);
}

#[test]
fn detects_short_form_build_path() {
    let yaml = "\
services:
  web:
    build: .
";
    assert!(run(yaml).has_build);
}

#[test]
fn no_build_for_image_only_service() {
    let yaml = "\
services:
  web:
    image: nginx:latest
    ports:
      - '8080:80'
";
    assert!(!run(yaml).has_build);
}

#[test]
fn no_build_when_services_section_absent() {
    let yaml = "version: '3.9'\n";
    assert!(!run(yaml).has_build);
}

#[test]
fn no_build_for_top_level_build_outside_services() {
    let yaml = "\
build: ignored
services:
  web:
    image: nginx:latest
";
    assert!(!run(yaml).has_build);
}

#[test]
fn detects_build_on_one_of_many_services() {
    let yaml = "\
services:
  web:
    image: nginx:latest
  api:
    build:
      context: ./api
";
    assert!(run(yaml).has_build);
}

#[test]
fn non_compose_path_returns_passthrough_with_no_build() {
    let template_values = serde_json::json!({});
    let node = NodeRecord::Local();
    let outcome = maybe_inject_compose_labels(
        &PathBuf::from("README.md"),
        "anything",
        &template_values,
        &node,
    )
    .unwrap();
    assert_eq!(outcome.content, "anything");
    assert!(!outcome.has_build);
}

#[test]
fn is_docker_compose_file_recognizes_yml_and_yaml() {
    assert!(is_docker_compose_file(&PathBuf::from("docker-compose.yml")));
    assert!(is_docker_compose_file(&PathBuf::from(
        "docker-compose.yaml"
    )));
    assert!(!is_docker_compose_file(&PathBuf::from("README.md")));
}
