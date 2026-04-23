use super::ExecutionOutput;

#[test]
fn execution_output_buffers_lines_in_order() {
    let output = ExecutionOutput::buffered();
    output.line("Starting check...");
    output.line("Check completed.");
    output.error_line("warning line");

    assert_eq!(
        output.snapshot(),
        "Starting check...\nCheck completed.\nwarning line"
    );
}
