use super::ExecutionOutput;
use tokio::sync::broadcast::error::TryRecvError;

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

#[test]
fn streaming_broadcasts_lines() {
    let out = ExecutionOutput::streaming();
    let mut rx = out.subscribe().expect("subscribed");
    out.line("first");
    out.line("second");
    assert_eq!(rx.try_recv().unwrap(), "first");
    assert_eq!(rx.try_recv().unwrap(), "second");
    assert!(matches!(rx.try_recv().unwrap_err(), TryRecvError::Empty));
}

#[test]
fn buffered_does_not_subscribe() {
    let out = ExecutionOutput::buffered();
    assert!(out.subscribe().is_none());
}

#[test]
fn streaming_keeps_snapshot() {
    let out = ExecutionOutput::streaming();
    out.line("a");
    out.line("b");
    assert_eq!(out.snapshot(), "a\nb");
}

#[test]
fn streaming_broadcasts_error_lines() {
    let out = ExecutionOutput::streaming();
    let mut rx = out.subscribe().expect("subscribed");
    out.error_line("oops");
    assert_eq!(rx.try_recv().unwrap(), "oops");
}

#[test]
fn streaming_late_subscribe_misses_prior_lines() {
    let out = ExecutionOutput::streaming();
    out.line("before");
    let mut rx = out.subscribe().expect("subscribed");
    out.line("after");
    assert_eq!(rx.try_recv().unwrap(), "after");
    assert!(matches!(rx.try_recv().unwrap_err(), TryRecvError::Empty));
    assert_eq!(out.snapshot(), "before\nafter");
}
