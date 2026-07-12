mod harness;

use std::time::Duration;

use crate::consumer::backoff::Backoff;
use harness::{Rebind, ScriptedSource, Step, drive, instant_backoff};

#[tokio::test]
async fn transient_stream_error_rebinds_and_keeps_processing() {
    let source = ScriptedSource::new(
        [Step::Frame(1), Step::Heartbeat, Step::Frame(2)],
        [Rebind::Ok],
    );
    let report = drive(source, instant_backoff()).await;

    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.seen, vec![1, 2]);
    assert_eq!(report.rebind_calls, 1);
}

#[tokio::test]
async fn a_deleted_durable_is_recreated_by_rebind_and_the_loop_resumes() {
    let source = ScriptedSource::new(
        [
            Step::Frame(1),
            Step::Heartbeat,
            Step::Frame(2),
            Step::Heartbeat,
            Step::Frame(3),
        ],
        [Rebind::Ok, Rebind::Ok],
    );
    let report = drive(source, instant_backoff()).await;

    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.seen, vec![1, 2, 3]);
    assert_eq!(report.rebind_calls, 2);
}

#[tokio::test]
async fn a_permanent_rebind_error_terminates_the_loop() {
    let source = ScriptedSource::new([Step::Frame(1), Step::Heartbeat], [Rebind::Terminal]);
    let report = drive(source, instant_backoff()).await;

    assert!(
        report.result.is_err(),
        "a permanently gone consumer must terminate the loop"
    );
}

#[tokio::test]
async fn a_rebind_that_finds_no_stream_terminates_the_loop() {
    let source = ScriptedSource::new([Step::Frame(1), Step::Heartbeat], [Rebind::NoStream]);
    let report = drive(source, instant_backoff()).await;

    assert!(
        report.result.is_err(),
        "a missing stream during re-bind is fail-loud terminal"
    );
}

#[tokio::test]
async fn a_terminal_rebind_error_after_transient_retries_terminates_the_loop() {
    let source = ScriptedSource::new(
        [Step::Frame(1), Step::Heartbeat],
        [Rebind::Heartbeat, Rebind::Heartbeat, Rebind::Terminal],
    );
    let report = drive(source, instant_backoff()).await;

    assert!(report.result.is_err());
    assert_eq!(report.rebind_calls, 3);
}

#[tokio::test]
async fn repeated_heartbeat_misses_retry_indefinitely_and_resume() {
    let mut rebinds: Vec<Rebind> = (0..20).map(|_| Rebind::Heartbeat).collect();
    rebinds.push(Rebind::Ok);
    let source = ScriptedSource::new([Step::Frame(1), Step::Heartbeat, Step::Frame(2)], rebinds);
    let report = drive(source, instant_backoff()).await;

    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.seen, vec![1, 2]);
    assert_eq!(report.rebind_calls, 21);
}

#[tokio::test]
async fn repeated_other_failures_exhaust_the_budget_and_surface_err() {
    let rebinds: Vec<Rebind> = (0..11).map(|_| Rebind::Other).collect();
    let source = ScriptedSource::new([Step::Frame(1), Step::Heartbeat], rebinds);
    let report = drive(source, instant_backoff()).await;

    assert!(
        report.result.is_err(),
        "sustained Other-class failures must fail loud once the budget is spent"
    );
    assert_eq!(report.rebind_calls, 11);
}

#[tokio::test]
async fn a_successful_delivery_resets_the_other_budget() {
    let mut steps = Vec::new();
    for id in 0..12 {
        steps.push(Step::Frame(id));
        steps.push(Step::OtherError);
    }
    let rebinds: Vec<Rebind> = (0..12).map(|_| Rebind::Ok).collect();
    let source = ScriptedSource::new(steps, rebinds);
    let report = drive(source, instant_backoff()).await;

    assert!(
        report.result.is_ok(),
        "a delivered frame between Other failures must reset the budget: {:?}",
        report.result
    );
    assert_eq!(report.seen, (0..12).collect::<Vec<_>>());
}

#[tokio::test]
async fn an_undecodable_payload_terms_and_the_loop_resumes() {
    let source = ScriptedSource::new([Step::BadFrame, Step::Frame(7)], []);
    let report = drive(source, instant_backoff()).await;

    assert!(report.result.is_ok(), "{:?}", report.result);
    assert_eq!(report.seen, vec![7]);
    assert_eq!(report.poison, 1);
}

#[tokio::test(start_paused = true)]
async fn recover_sleeps_with_backoff_between_attempts() {
    let source = ScriptedSource::new(
        [Step::Heartbeat],
        [Rebind::Heartbeat, Rebind::Heartbeat, Rebind::Ok],
    );
    let backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));

    let start = tokio::time::Instant::now();
    let report = drive(source, backoff).await;
    let elapsed = start.elapsed();

    assert!(report.result.is_ok(), "{:?}", report.result);
    assert!(
        elapsed >= Duration::from_millis(700),
        "expected >=700ms of cumulative backoff, got {elapsed:?}"
    );
}
