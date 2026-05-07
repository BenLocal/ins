use super::*;
use crate::execution_output::ExecutionOutput;

#[test]
fn id_format() {
    let id = next_job_id();
    // YYYYMMDD-HHMMSS-XXXXXXXX → 8 + 1 + 6 + 1 + 8 = 24
    assert_eq!(id.len(), 24);
    assert_eq!(&id[8..9], "-");
    assert_eq!(&id[15..16], "-");
}

#[tokio::test]
async fn output_streaming_emits_done() {
    let out = ExecutionOutput::streaming();
    let mut rx = out.subscribe().unwrap();
    out.line("hello");
    out.line("[ins:done] ok");
    assert_eq!(rx.recv().await.unwrap(), "hello");
    assert_eq!(rx.recv().await.unwrap(), "[ins:done] ok");
}
