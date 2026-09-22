use super::Args;
use clap::CommandFactory;

#[test]
fn usage_and_gate_telemetry_have_stable_help_descriptions() {
    let mut command = Args::command();
    let mut help = Vec::new();
    command.write_long_help(&mut help).expect("render help");
    let help = String::from_utf8(help).expect("UTF-8 help");

    assert!(help.contains("Show usage limits for connected providers"));
    assert!(
        help.contains("Report actual consent-gate prompts and decisions from the last seven days")
    );
}
