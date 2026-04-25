use std::{
    collections::BTreeMap,
    env,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use serde_json::json;
use tokio::fs;

use crate::app::parse::{expand_env_vars, load_app_record};

fn no_env() -> BTreeMap<String, String> {
    BTreeMap::new()
}

const QA_TEMPLATE: &str = include_str!("../../template/qa.yaml");

#[tokio::test]
async fn load_app_record_parses_template_yaml() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-template");
    let qa_file = test_dir.join("qa.yaml");

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, QA_TEMPLATE).await?;

    let record = load_app_record(&qa_file, &no_env()).await?;

    assert_eq!(record.name, "<name>");
    assert_eq!(record.version.as_deref(), Some("<version>"));
    assert_eq!(record.description.as_deref(), Some("<description>"));
    assert_eq!(record.author_name.as_deref(), Some("<author_name>"));
    assert_eq!(record.author_email.as_deref(), Some("<author_email>"));
    assert_eq!(record.dependencies, vec!["service_name"]);
    assert_eq!(record.before.shell.as_deref(), Some("bash"));
    assert_eq!(record.before.script.as_deref(), Some("./before.sh"));
    assert_eq!(record.after.shell.as_deref(), Some("bash"));
    assert_eq!(record.after.script.as_deref(), Some("./after.sh"));
    assert_eq!(record.values.len(), 1);
    assert_eq!(record.values[0].name, "<name>");
    assert_eq!(record.values[0].value_type, "string");
    assert_eq!(
        record.values[0].description.as_deref(),
        Some("<description>")
    );
    assert_eq!(record.values[0].options.len(), 1);
    assert_eq!(record.values[0].options[0].name, "<name>");
    assert_eq!(
        record.values[0].options[0].description.as_deref(),
        Some("<description>")
    );
    assert_eq!(record.values[0].options[0].value, Some(json!("<value>")));

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[tokio::test]
async fn load_app_record_parses_value_and_default_fields() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-values");
    let qa_file = test_dir.join("qa.yaml");
    let qa = r#"
name: demo
version: 1.2.3
dependencies:
  - redis
values:
  - name: replicas
    type: number
    default: 3
  - name: feature_enabled
    type: boolean
    value: true
"#;

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, qa.trim_start()).await?;

    let record = load_app_record(&qa_file, &no_env()).await?;

    assert_eq!(record.version.as_deref(), Some("1.2.3"));
    assert_eq!(record.dependencies, vec!["redis"]);
    assert_eq!(record.values.len(), 2);
    assert_eq!(record.values[0].default, Some(json!(3)));
    assert_eq!(record.values[1].value, Some(json!(true)));

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[tokio::test]
async fn load_app_record_collects_sibling_files_and_directories() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-files");
    let qa_file = test_dir.join("qa.yaml");
    let child_dir = test_dir.join("scripts");
    let child_file = test_dir.join("README.md");

    fs::create_dir_all(&child_dir).await?;
    fs::write(&qa_file, QA_TEMPLATE).await?;
    fs::write(&child_file, "demo").await?;

    let record = load_app_record(&qa_file, &no_env()).await?;
    let files = record.files.expect("files should be populated");

    // `qa.yaml` itself is intentionally excluded from sibling file list.
    assert_eq!(files.len(), 2);
    assert_eq!(files[0].name, "README.md");
    assert_eq!(files[0].path, child_file.display().to_string());
    assert!(!files[0].is_dir);
    assert_eq!(files[1].name, "scripts");
    assert_eq!(files[1].path, child_dir.display().to_string());
    assert!(files[1].is_dir);

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[tokio::test]
async fn load_app_record_defaults_dependencies_to_empty() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-dependencies-empty");
    let qa_file = test_dir.join("qa.yaml");
    let qa = r#"
name: demo
values: []
"#;

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, qa.trim_start()).await?;

    let record = load_app_record(&qa_file, &no_env()).await?;

    assert!(record.dependencies.is_empty());

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[test]
fn expand_env_substitutes_set_var_with_braces() {
    // SAFETY: unique key per test so concurrent tests don't clobber.
    unsafe { env::set_var("INS_TEST_PARSE_ENV_SET", "hello") };
    let out = expand_env_vars("value: ${INS_TEST_PARSE_ENV_SET}", &no_env()).unwrap();
    assert_eq!(out, "value: hello");
    unsafe { env::remove_var("INS_TEST_PARSE_ENV_SET") };
}

#[test]
fn expand_env_uses_fallback_when_var_unset() {
    unsafe { env::remove_var("INS_TEST_PARSE_ENV_UNSET") };
    let out = expand_env_vars("value: ${INS_TEST_PARSE_ENV_UNSET:-fallback}", &no_env()).unwrap();
    assert_eq!(out, "value: fallback");
}

#[test]
fn expand_env_errors_on_unset_without_fallback() {
    unsafe { env::remove_var("INS_TEST_PARSE_ENV_MISSING") };
    let err = expand_env_vars("value: ${INS_TEST_PARSE_ENV_MISSING}", &no_env()).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("INS_TEST_PARSE_ENV_MISSING"),
        "expected env-var name in error: {msg}"
    );
}

#[test]
fn expand_env_escapes_double_dollar_to_literal() {
    let out = expand_env_vars("command: mysqladmin -p$$MYSQL_PASSWORD", &no_env()).unwrap();
    assert_eq!(out, "command: mysqladmin -p$MYSQL_PASSWORD");
}

#[test]
fn expand_env_leaves_jinja_expressions_untouched() {
    let out = expand_env_vars("port: {{ vars.port | default(3306) }}", &no_env()).unwrap();
    assert_eq!(out, "port: {{ vars.port | default(3306) }}");
}

#[test]
fn expand_env_errors_on_unterminated_reference() {
    let err = expand_env_vars("value: ${OPEN", &no_env()).unwrap_err();
    assert!(format!("{err}").contains("unterminated"));
}

#[test]
fn expand_env_prefers_extra_env_over_process_env() {
    unsafe { env::set_var("INS_TEST_CONFIG_OVERRIDE", "from-process") };
    let mut extra = BTreeMap::new();
    extra.insert("INS_TEST_CONFIG_OVERRIDE".into(), "from-config".into());
    let out = expand_env_vars("value: ${INS_TEST_CONFIG_OVERRIDE}", &extra).unwrap();
    assert_eq!(out, "value: from-config");
    unsafe { env::remove_var("INS_TEST_CONFIG_OVERRIDE") };
}

#[test]
fn expand_env_falls_back_to_process_env_when_absent_in_extra() {
    unsafe { env::set_var("INS_TEST_PROCESS_ONLY", "proc-value") };
    let out = expand_env_vars("value: ${INS_TEST_PROCESS_ONLY}", &no_env()).unwrap();
    assert_eq!(out, "value: proc-value");
    unsafe { env::remove_var("INS_TEST_PROCESS_ONLY") };
}

#[test]
fn expand_env_uses_extra_env_when_process_unset() {
    unsafe { env::remove_var("INS_TEST_CONFIG_ONLY") };
    let mut extra = BTreeMap::new();
    extra.insert("INS_TEST_CONFIG_ONLY".into(), "cfg-only".into());
    let out = expand_env_vars("value: ${INS_TEST_CONFIG_ONLY}", &extra).unwrap();
    assert_eq!(out, "value: cfg-only");
}

#[tokio::test]
async fn load_app_record_parses_optional_order_field() -> anyhow::Result<()> {
    let test_dir = unique_test_dir("parse-order");
    let qa_file = test_dir.join("qa.yaml");
    let qa = r#"
name: demo
order: 5
values: []
"#;

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, qa.trim_start()).await?;

    let record = load_app_record(&qa_file, &no_env()).await?;
    assert_eq!(record.order, Some(5));

    fs::remove_dir_all(&test_dir).await?;
    Ok(())
}

#[test]
fn sort_apps_puts_ordered_first_then_alphabetical_unordered() {
    use crate::app::types::{AppRecord, sort_apps_for_display};
    let mk = |name: &str, order: Option<i64>| AppRecord {
        name: name.into(),
        order,
        ..Default::default()
    };
    let mut apps = vec![
        mk("zebra", None),
        mk("apple", None),
        mk("mysql", Some(10)),
        mk("redis", Some(5)),
        mk("nginx", Some(10)),
    ];
    sort_apps_for_display(&mut apps);
    let names: Vec<_> = apps.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(names, vec!["redis", "mysql", "nginx", "apple", "zebra"]);
}

#[tokio::test]
async fn load_app_record_applies_env_var_substitution() -> anyhow::Result<()> {
    unsafe { env::set_var("INS_TEST_LOAD_PW", "super-secret") };
    let test_dir = unique_test_dir("parse-envvars");
    let qa_file = test_dir.join("qa.yaml");
    let qa = r#"
name: demo
values:
  - name: password
    type: string
    default: "${INS_TEST_LOAD_PW}"
  - name: port
    type: number
    default: 3306
"#;

    fs::create_dir_all(&test_dir).await?;
    fs::write(&qa_file, qa.trim_start()).await?;

    let record = load_app_record(&qa_file, &no_env()).await?;
    assert_eq!(record.values[0].default, Some(json!("super-secret")));

    fs::remove_dir_all(&test_dir).await?;
    unsafe { env::remove_var("INS_TEST_LOAD_PW") };
    Ok(())
}

fn unique_test_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before unix epoch")
        .as_nanos();
    env::temp_dir().join(format!("ins-{name}-{}-{nanos}", std::process::id()))
}

#[tokio::test]
async fn parsed_dependencies_splits_namespace_prefixes() {
    use crate::app::dependency::{DEFAULT_NAMESPACE, DependencyRef};
    use crate::app::types::AppRecord;

    let app = AppRecord {
        name: "web".into(),
        dependencies: vec!["redis".into(), ":mysql".into(), "staging:cache".into()],
        ..AppRecord::default()
    };

    let deps = app.parsed_dependencies().expect("parse");
    assert_eq!(deps.len(), 3);
    assert_eq!(
        deps[0],
        DependencyRef {
            namespace: DEFAULT_NAMESPACE.into(),
            service: "redis".into(),
            explicit_namespace: false,
        }
    );
    assert_eq!(deps[1].service, "mysql");
    assert!(!deps[1].explicit_namespace);
    assert_eq!(deps[2].namespace, "staging");
    assert!(deps[2].explicit_namespace);
}
