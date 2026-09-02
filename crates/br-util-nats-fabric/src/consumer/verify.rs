use crate::classify::classify_get_stream;
use crate::consumer::coverage::subject_covered;
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

impl Fabric {
    pub async fn verify_command_durable(
        &self,
        coords: &CommandCoords,
        _durable: &str,
    ) -> Result<(), FabricError> {
        verify_stream_covers(self.context(), INTEGRATION_CMD, &coords.subject()).await
    }

    pub async fn verify_event_durable(
        &self,
        coords: &EventCoords,
        _durable: &str,
    ) -> Result<(), FabricError> {
        verify_stream_covers(self.context(), INTEGRATION_EVT, &coords.subject()).await
    }
}

async fn verify_stream_covers(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &'static str,
    subject: &str,
) -> Result<(), FabricError> {
    let stream = jetstream
        .get_stream(stream_name)
        .await
        .map_err(|e| FabricError::consume(classify_get_stream(&e), e.to_string()))?;

    let configured = &stream.cached_info().config.subjects;
    if subject_covered(configured, subject) {
        return Ok(());
    }

    Err(FabricError::SubjectNotCovered {
        stream: stream_name,
        subject: subject.to_string(),
        configured: configured.clone(),
    })
}
