use super::{Command, InsCli};
use clap::Parser;

#[test]
fn tui_command_parses_from_cli() {
    let cli = InsCli::try_parse_from(["ins", "tui"]).expect("tui command should parse");
    assert!(matches!(cli.command, Some(Command::Tui(_))));
}

#[test]
fn deploy_command_parses_repeated_value_flags() {
    let cli = InsCli::try_parse_from([
        "ins",
        "deploy",
        "-w",
        "/tmp/workspace",
        "-v",
        "image=nginx:1.27",
        "-v",
        "port=8080",
    ])
    .expect("deploy command should parse");

    let Some(Command::Deploy(args)) = cli.command else {
        panic!("expected deploy command");
    };

    assert_eq!(args.pipeline.values, vec!["image=nginx:1.27", "port=8080"]);
}
