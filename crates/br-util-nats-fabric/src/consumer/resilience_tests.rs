use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind};
use serde::Deserialize;

use br_core_integration::MessageOutcome;

use crate::classify::classify_messages_error;
use crate::consumer::backoff::Backoff;
use crate::consumer::run::{Delivery, run_inner};
use crate::consumer::source::{MessageSource, SourceFrame};
use crate::error::FabricError;

fn instant_backoff() -> Backoff {
    Backoff::new(Duration::ZERO, Duration::ZERO)
}

#[derive(Deserialize)]
struct TestEnvelope {
    id: u32,
}

struct TestFrame {
    subject: String,
    payload: Vec<u8>,
}

#[async_trait::async_trait]
impl SourceFrame for TestFrame {
    fn subject(&self) -> &str {
        &self.subject
    }

    fn payload(&self) -> &[u8] {
        &self.payload
    }

    async fn ack(self, _kind: AckKind) -> Result<(), String> {
        Ok(())
    }
}

enum Step {
    Frame(u32),
    TransientError,
}

enum Rebind {
    Ok,
    Transient,
    Gone,
}

struct ScriptedSource {
    steps: VecDeque<Step>,
    rebinds: VecDeque<Rebind>,
    rebind_calls: Arc<Mutex<usize>>,
}

impl ScriptedSource {
    fn new(
        steps: impl IntoIterator<Item = Step>,
        rebinds: impl IntoIterator<Item = Rebind>,
    ) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            rebinds: rebinds.into_iter().collect(),
            rebind_calls: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl MessageSource for ScriptedSource {
    type Frame = TestFrame;

    async fn next(&mut self) -> Option<Result<Self::Frame, FabricError>> {
        match self.steps.pop_front() {
            None => None,
            Some(Step::Frame(id)) => {
                let payload = serde_json::to_vec(&serde_json::json!({ "id": id })).unwrap();
                Some(Ok(TestFrame {
                    subject: "integration.evt.identity.user.created.v1".to_string(),
                    payload,
                }))
            }
            Some(Step::TransientError) => Some(Err(FabricError::consume(
                classify_messages_error(&MessagesError::new(MessagesErrorKind::MissingHeartbeat)),
                "missing heartbeat",
            ))),
        }
    }

    async fn rebind(&mut self) -> Result<(), FabricError> {
        *self.rebind_calls.lock().unwrap() += 1;
        match self.rebinds.pop_front().unwrap_or(Rebind::Ok) {
            Rebind::Ok => Ok(()),
            Rebind::Transient => Err(FabricError::consume(
                classify_messages_error(&MessagesError::new(MessagesErrorKind::MissingHeartbeat)),
                "rebind hit a transient heartbeat gap",
            )),
            Rebind::Gone => Err(FabricError::consume(
                classify_messages_error(&MessagesError::new(MessagesErrorKind::ConsumerDeleted)),
                "consumer confirmed gone on re-verification",
            )),
        }
    }
}

#[tokio::test]
async fn transient_stream_error_rebinds_and_keeps_processing() {
    let source = ScriptedSource::new(
        [Step::Frame(1), Step::TransientError, Step::Frame(2)],
        [Rebind::Ok],
    );
    let rebind_calls = source.rebind_calls.clone();

    let seen = Arc::new(Mutex::new(Vec::<u32>::new()));
    let seen_handle = seen.clone();
    let mut handler = move |delivery: Delivery<TestEnvelope>| {
        let seen_handle = seen_handle.clone();
        async move {
            seen_handle.lock().unwrap().push(delivery.envelope.id);
            MessageOutcome::Ack
        }
    };
    let mut on_poison = |_error: FabricError| {};

    let result = run_inner::<TestEnvelope, _, _, _, _>(
        source,
        &mut handler,
        &mut on_poison,
        instant_backoff(),
    )
    .await;

    assert!(
        result.is_ok(),
        "a transient stream error must not terminate the run loop: {result:?}"
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![1, 2],
        "the loop must process messages delivered after re-binding on a transient error"
    );
    assert_eq!(
        *rebind_calls.lock().unwrap(),
        1,
        "the loop must re-bind the durable exactly once to recover from the transient error"
    );
}

#[tokio::test]
async fn a_confirmed_gone_consumer_terminates_the_loop_with_err() {
    let source = ScriptedSource::new([Step::Frame(1), Step::TransientError], [Rebind::Gone]);

    let mut handler = |_delivery: Delivery<TestEnvelope>| async { MessageOutcome::Ack };
    let mut on_poison = |_error: FabricError| {};

    let result = run_inner::<TestEnvelope, _, _, _, _>(
        source,
        &mut handler,
        &mut on_poison,
        instant_backoff(),
    )
    .await;

    assert!(
        result.is_err(),
        "a consumer confirmed gone after re-verification must terminate the loop with Err"
    );
}

#[tokio::test]
async fn repeated_transient_rebind_failures_keep_retrying_past_a_naive_budget() {
    let source = ScriptedSource::new(
        [Step::Frame(1), Step::TransientError, Step::Frame(2)],
        [
            Rebind::Transient,
            Rebind::Transient,
            Rebind::Transient,
            Rebind::Transient,
            Rebind::Transient,
            Rebind::Ok,
        ],
    );
    let rebind_calls = source.rebind_calls.clone();

    let seen = Arc::new(Mutex::new(Vec::<u32>::new()));
    let seen_handle = seen.clone();
    let mut handler = move |delivery: Delivery<TestEnvelope>| {
        let seen_handle = seen_handle.clone();
        async move {
            seen_handle.lock().unwrap().push(delivery.envelope.id);
            MessageOutcome::Ack
        }
    };
    let mut on_poison = |_error: FabricError| {};

    let result = run_inner::<TestEnvelope, _, _, _, _>(
        source,
        &mut handler,
        &mut on_poison,
        instant_backoff(),
    )
    .await;

    assert!(
        result.is_ok(),
        "sustained transient rebind failures must not terminate the loop: {result:?}"
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![1, 2],
        "the loop must resume processing once a rebind finally succeeds"
    );
    assert_eq!(
        *rebind_calls.lock().unwrap(),
        6,
        "the loop must keep re-binding through every transient failure until one succeeds"
    );
}

#[tokio::test]
async fn a_transient_rebind_that_turns_out_gone_terminates_the_loop() {
    let source = ScriptedSource::new(
        [Step::Frame(1), Step::TransientError],
        [Rebind::Transient, Rebind::Transient, Rebind::Gone],
    );
    let rebind_calls = source.rebind_calls.clone();

    let mut handler = |_delivery: Delivery<TestEnvelope>| async { MessageOutcome::Ack };
    let mut on_poison = |_error: FabricError| {};

    let result = run_inner::<TestEnvelope, _, _, _, _>(
        source,
        &mut handler,
        &mut on_poison,
        instant_backoff(),
    )
    .await;

    assert!(
        result.is_err(),
        "a rebind that surfaces a confirmed-gone consumer must terminate the loop with Err"
    );
    assert_eq!(
        *rebind_calls.lock().unwrap(),
        3,
        "the loop retries transient rebind failures, then stops on the confirmed-gone one"
    );
}
