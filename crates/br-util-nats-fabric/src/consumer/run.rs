use std::future::Future;

use async_nats::jetstream::AckKind;
use serde::de::DeserializeOwned;

use br_core_integration::{IntegrationCommand, IntegrationEvent, MessageOutcome};

use crate::consumer::backoff::Backoff;
use crate::consumer::bind::ensure_durable;
use crate::consumer::config::ConsumerTuning;
use crate::consumer::source::{MessageSource, PullSource, SourceFrame};
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::{ConsumeErrorKind, FabricError};
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

#[non_exhaustive]
pub struct Delivery<E> {
    pub subject: String,
    pub envelope: E,
}

impl Fabric {
    pub async fn run_commands<T, H, HFut, P>(
        &self,
        coords: &CommandCoords,
        durable: &str,
        handler: H,
        on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        self.run_commands_with(
            coords,
            durable,
            &ConsumerTuning::default(),
            handler,
            on_poison,
        )
        .await
    }

    pub async fn run_commands_with<T, H, HFut, P>(
        &self,
        coords: &CommandCoords,
        durable: &str,
        tuning: &ConsumerTuning,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        let filter = coords.subject();
        let consumer =
            ensure_durable(self.context(), INTEGRATION_CMD, durable, &filter, tuning).await?;
        let source = PullSource::open(
            self.context().clone(),
            consumer,
            INTEGRATION_CMD,
            durable.to_string(),
            filter,
            *tuning,
        )
        .await?;
        run_inner::<IntegrationCommand<T>, _, _, _, _>(
            source,
            &mut handler,
            &mut on_poison,
            Backoff::production(),
        )
        .await
    }

    pub async fn run_events<T, H, HFut, P>(
        &self,
        coords: &EventCoords,
        durable: &str,
        handler: H,
        on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        self.run_events_with(
            coords,
            durable,
            &ConsumerTuning::default(),
            handler,
            on_poison,
        )
        .await
    }

    pub async fn run_events_with<T, H, HFut, P>(
        &self,
        coords: &EventCoords,
        durable: &str,
        tuning: &ConsumerTuning,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        let filter = coords.subject();
        let consumer =
            ensure_durable(self.context(), INTEGRATION_EVT, durable, &filter, tuning).await?;
        let source = PullSource::open(
            self.context().clone(),
            consumer,
            INTEGRATION_EVT,
            durable.to_string(),
            filter,
            *tuning,
        )
        .await?;
        run_inner::<IntegrationEvent<T>, _, _, _, _>(
            source,
            &mut handler,
            &mut on_poison,
            Backoff::production(),
        )
        .await
    }
}

pub(crate) async fn run_inner<E, S, H, HFut, P>(
    mut source: S,
    handler: &mut H,
    on_poison: &mut P,
    mut backoff: Backoff,
) -> Result<(), FabricError>
where
    E: DeserializeOwned,
    S: MessageSource,
    H: FnMut(Delivery<E>) -> HFut,
    HFut: Future<Output = MessageOutcome>,
    P: FnMut(FabricError),
{
    loop {
        match source.next().await {
            None => return Ok(()),
            Some(Err(error)) => {
                if should_recover(&error) {
                    recover(&mut source, &error, &mut backoff).await?;
                    continue;
                }
                return Err(error);
            }
            Some(Ok(frame)) => {
                let subject = frame.subject().to_string();
                match serde_json::from_slice::<E>(frame.payload()) {
                    Ok(envelope) => {
                        let outcome = handler(Delivery {
                            subject: subject.clone(),
                            envelope,
                        })
                        .await;
                        apply_outcome(frame, &subject, outcome).await;
                    }
                    Err(decode_err) => {
                        let _ = frame.ack(AckKind::Term).await;
                        on_poison(FabricError::decode(subject, &decode_err));
                    }
                }
            }
        }
    }
}

async fn recover<S: MessageSource>(
    source: &mut S,
    trigger: &FabricError,
    backoff: &mut Backoff,
) -> Result<(), FabricError> {
    let kind = consume_kind(trigger);
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let delay = backoff.sleep().await;
        tracing::warn!(
            consume_kind = ?kind,
            attempt,
            delay_ms = delay.as_millis() as u64,
            "re-binding durable consumer after a transient stream error"
        );
        match source.rebind().await {
            Ok(()) => {
                backoff.reset();
                return Ok(());
            }
            Err(rebind_err) if should_recover(&rebind_err) => continue,
            Err(rebind_err) => return Err(rebind_err),
        }
    }
}

fn should_recover(error: &FabricError) -> bool {
    matches!(
        error,
        FabricError::Consume {
            kind: ConsumeErrorKind::HeartbeatMissed | ConsumeErrorKind::Other,
            ..
        }
    )
}

fn consume_kind(error: &FabricError) -> Option<ConsumeErrorKind> {
    match error {
        FabricError::Consume { kind, .. } => Some(*kind),
        _ => None,
    }
}

fn ack_kind(outcome: MessageOutcome) -> AckKind {
    match outcome {
        MessageOutcome::Ack => AckKind::Ack,
        MessageOutcome::Nak(delay) => AckKind::Nak(delay),
        MessageOutcome::Term => AckKind::Term,
        _ => AckKind::Nak(None),
    }
}

async fn apply_outcome<F: SourceFrame>(frame: F, subject: &str, outcome: MessageOutcome) {
    if let Err(err) = frame.ack(ack_kind(outcome)).await {
        tracing::warn!(
            error = %err,
            subject = %subject,
            ?outcome,
            "failed to send ack for handled message; it may be redelivered"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ack_kind;
    use async_nats::jetstream::AckKind;
    use br_core_integration::MessageOutcome;
    use std::time::Duration;

    #[test]
    fn maps_known_outcomes_to_their_ack_kind() {
        assert!(matches!(ack_kind(MessageOutcome::Ack), AckKind::Ack));
        assert!(matches!(
            ack_kind(MessageOutcome::Nak(None)),
            AckKind::Nak(None)
        ));
        assert!(matches!(
            ack_kind(MessageOutcome::Nak(Some(Duration::from_secs(2)))),
            AckKind::Nak(Some(_))
        ));
        assert!(matches!(ack_kind(MessageOutcome::Term), AckKind::Term));
    }
}
