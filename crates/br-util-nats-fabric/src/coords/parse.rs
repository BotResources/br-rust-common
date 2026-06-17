use br_core_integration::{Aggregate, Bc, CoordError, EventCoords, PastFact};

use crate::coords::render::{EVT_TOKEN, INTEGRATION_PREFIX};

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum EventSubjectParseError {
    #[error("event subject {subject:?} is not the 6-segment integration.evt grammar")]
    Shape { subject: String },
    #[error("event subject version segment {segment:?} is not 'v<u8>'")]
    Version { segment: String },
    #[error(transparent)]
    Coord(#[from] CoordError),
}

pub fn parse_event_subject(subject: &str) -> Result<EventCoords, EventSubjectParseError> {
    let parts: Vec<&str> = subject.split('.').collect();
    let [prefix, token, producer, aggregate, fact, version] = parts.as_slice() else {
        return Err(EventSubjectParseError::Shape {
            subject: subject.to_string(),
        });
    };
    if *prefix != INTEGRATION_PREFIX || *token != EVT_TOKEN {
        return Err(EventSubjectParseError::Shape {
            subject: subject.to_string(),
        });
    }
    let version = version
        .strip_prefix('v')
        .and_then(|n| n.parse::<u8>().ok())
        .ok_or_else(|| EventSubjectParseError::Version {
            segment: version.to_string(),
        })?;
    Ok(EventCoords {
        producer: Bc::new(*producer)?,
        aggregate: Aggregate::new(*aggregate)?,
        fact: PastFact::new(*fact)?,
        version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::render::IntegrationSubject;

    #[test]
    fn round_trips_a_rendered_event_subject() {
        let coords = EventCoords {
            producer: Bc::new("identity").unwrap(),
            aggregate: Aggregate::new("user").unwrap(),
            fact: PastFact::new("created").unwrap(),
            version: 3,
        };
        let parsed = parse_event_subject(&coords.subject()).unwrap();
        assert_eq!(parsed, coords);
    }

    #[test]
    fn rejects_a_command_subject() {
        let err =
            parse_event_subject("integration.cmd.notifier.notification.deliver.v1").unwrap_err();
        assert!(matches!(err, EventSubjectParseError::Shape { .. }));
    }

    #[test]
    fn rejects_a_wrong_arity_subject() {
        assert!(matches!(
            parse_event_subject("integration.evt.identity.user.created").unwrap_err(),
            EventSubjectParseError::Shape { .. }
        ));
    }

    #[test]
    fn rejects_a_bad_version_segment() {
        assert!(matches!(
            parse_event_subject("integration.evt.identity.user.created.x1").unwrap_err(),
            EventSubjectParseError::Version { .. }
        ));
    }

    #[test]
    fn rejects_a_foreign_prefix() {
        assert!(matches!(
            parse_event_subject("identity.evt.identity.user.created.v1").unwrap_err(),
            EventSubjectParseError::Shape { .. }
        ));
    }
}
