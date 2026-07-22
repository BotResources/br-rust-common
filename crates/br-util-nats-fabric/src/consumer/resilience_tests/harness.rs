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
use crate::error::{ConsumeErrorKind, FabricError};

pub(super) fn instant_backoff() -> Backoff {
    Backoff::new(Duration::ZERO, Duration::ZERO)
}

fn heartbeat() -> FabricError {
    FabricError::consume(
        classify_messages_error(&MessagesError::new(MessagesErrorKind::MissingHeartbeat)),
        "missing heartbeat",
    )
}

fn other() -> FabricError {
    FabricError::consume(
        classify_messages_error(&MessagesError::new(MessagesErrorKind::Pull)),
        "transport hiccup",
    )
}

fn no_responders() -> FabricError {
    FabricError::consume(
        classify_messages_error(&MessagesError::new(MessagesErrorKind::NoResponders)),
        "no jetstream responders during broker restart",
    )
}

#[derive(Deserialize)]
struct TestEnvelope {
    id: u32,
}

pub(super) struct TestFrame {
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

pub(super) enum Step {
    Frame(u32),
    Heartbeat,
    NoResponders,
    OtherError,
    BadFrame,
}

pub(super) enum Rebind {
    Ok,
    Heartbeat,
    NoResponders,
    Other,
    NoStream,
    Terminal,
}

pub(super) struct ScriptedSource {
    steps: VecDeque<Step>,
    rebinds: VecDeque<Rebind>,
    rebind_calls: Arc<Mutex<usize>>,
}

impl ScriptedSource {
    pub(super) fn new(
        steps: impl IntoIterator<Item = Step>,
        rebinds: impl IntoIterator<Item = Rebind>,
    ) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            rebinds: rebinds.into_iter().collect(),
            rebind_calls: Arc::new(Mutex::new(0)),
        }
    }

    fn frame(id: u32) -> TestFrame {
        let payload = serde_json::to_vec(&serde_json::json!({ "id": id })).unwrap();
        TestFrame {
            subject: "integration.evt.identity.user.created.v1".to_string(),
            payload,
        }
    }
}

#[async_trait::async_trait]
impl MessageSource for ScriptedSource {
    type Frame = TestFrame;

    async fn next(&mut self) -> Option<Result<Self::Frame, FabricError>> {
        match self.steps.pop_front()? {
            Step::Frame(id) => Some(Ok(Self::frame(id))),
            Step::BadFrame => Some(Ok(TestFrame {
                subject: "integration.evt.identity.user.created.v1".to_string(),
                payload: b"not json".to_vec(),
            })),
            Step::Heartbeat => Some(Err(heartbeat())),
            Step::NoResponders => Some(Err(no_responders())),
            Step::OtherError => Some(Err(other())),
        }
    }

    async fn rebind(&mut self) -> Result<(), FabricError> {
        *self.rebind_calls.lock().unwrap() += 1;
        match self.rebinds.pop_front().unwrap_or(Rebind::Ok) {
            Rebind::Ok => Ok(()),
            Rebind::Heartbeat => Err(heartbeat()),
            Rebind::NoResponders => Err(no_responders()),
            Rebind::Other => Err(other()),
            Rebind::NoStream => Err(FabricError::consume(
                ConsumeErrorKind::NoStream,
                "stream vanished during re-bind",
            )),
            Rebind::Terminal => Err(FabricError::consume(
                classify_messages_error(&MessagesError::new(MessagesErrorKind::ConsumerDeleted)),
                "consumer confirmed gone on re-verification",
            )),
        }
    }
}

pub(super) struct Report {
    pub(super) result: Result<(), FabricError>,
    pub(super) seen: Vec<u32>,
    pub(super) rebind_calls: usize,
    pub(super) poison: usize,
}

pub(super) async fn drive(source: ScriptedSource, backoff: Backoff) -> Report {
    let rebind_calls = source.rebind_calls.clone();
    let seen = Arc::new(Mutex::new(Vec::<u32>::new()));
    let poison = Arc::new(Mutex::new(0usize));

    let seen_handle = seen.clone();
    let mut handler = move |delivery: Delivery<TestEnvelope>| {
        let seen_handle = seen_handle.clone();
        async move {
            seen_handle.lock().unwrap().push(delivery.envelope.id);
            MessageOutcome::Ack
        }
    };
    let poison_handle = poison.clone();
    let mut on_poison = move |_error: FabricError| {
        *poison_handle.lock().unwrap() += 1;
    };

    let result =
        run_inner::<TestEnvelope, _, _, _, _>(source, &mut handler, &mut on_poison, backoff).await;

    Report {
        result,
        seen: seen.lock().unwrap().clone(),
        rebind_calls: *rebind_calls.lock().unwrap(),
        poison: *poison.lock().unwrap(),
    }
}
