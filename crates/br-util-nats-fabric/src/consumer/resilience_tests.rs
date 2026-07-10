use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind};
use serde::Deserialize;

use br_core_integration::MessageOutcome;

use crate::classify::classify_messages_error;
use crate::consumer::run::{Delivery, run_inner};
use crate::consumer::source::{MessageSource, SourceFrame};
use crate::error::FabricError;

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

struct ScriptedSource {
    steps: VecDeque<Step>,
    rebind_ok: bool,
    rebind_calls: Arc<Mutex<usize>>,
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
        if self.rebind_ok {
            Ok(())
        } else {
            Err(FabricError::consume(
                classify_messages_error(&MessagesError::new(MessagesErrorKind::ConsumerDeleted)),
                "consumer confirmed gone on re-verification",
            ))
        }
    }
}

#[tokio::test]
async fn transient_stream_error_rebinds_and_keeps_processing() {
    let rebind_calls = Arc::new(Mutex::new(0));
    let source = ScriptedSource {
        steps: VecDeque::from([Step::Frame(1), Step::TransientError, Step::Frame(2)]),
        rebind_ok: true,
        rebind_calls: rebind_calls.clone(),
    };

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

    let result = run_inner::<TestEnvelope, _, _, _, _>(source, &mut handler, &mut on_poison).await;

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
    let source = ScriptedSource {
        steps: VecDeque::from([Step::Frame(1), Step::TransientError]),
        rebind_ok: false,
        rebind_calls: Arc::new(Mutex::new(0)),
    };

    let mut handler = |_delivery: Delivery<TestEnvelope>| async { MessageOutcome::Ack };
    let mut on_poison = |_error: FabricError| {};

    let result = run_inner::<TestEnvelope, _, _, _, _>(source, &mut handler, &mut on_poison).await;

    assert!(
        result.is_err(),
        "a consumer confirmed gone after re-verification must terminate the loop with Err"
    );
}
