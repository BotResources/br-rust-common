use std::fmt::Display;

use async_nats::jetstream::kv::{CreateErrorKind, Operation, Store, UpdateErrorKind};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::kv::codec::{decode, encode};
use crate::kv::key::KvKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Revision(u64);

impl Revision {
    pub(crate) fn new(value: u64) -> Self {
        Self(value)
    }

    pub(crate) fn get(&self) -> u64 {
        self.0
    }
}

pub(crate) fn map_revision_error(
    key: &KvKey,
    expected: Revision,
    kind: UpdateErrorKind,
    detail: impl Display,
) -> FabricError {
    match kind {
        UpdateErrorKind::WrongLastRevision => {
            FabricError::revision_conflict(key.as_str(), expected.get())
        }
        _ => FabricError::kv(detail),
    }
}

pub(crate) fn map_create_error(
    key: &KvKey,
    kind: CreateErrorKind,
    detail: impl Display,
) -> FabricError {
    match kind {
        CreateErrorKind::AlreadyExists => FabricError::key_already_exists(key.as_str()),
        _ => FabricError::kv(detail),
    }
}

pub(crate) async fn read_with_revision<V: DeserializeOwned>(
    kv: &Store,
    key: &KvKey,
) -> Result<Option<(V, Revision)>, FabricError> {
    let Some(entry) = kv.entry(key.as_str()).await.map_err(FabricError::kv)? else {
        return Ok(None);
    };
    if matches!(entry.operation, Operation::Delete | Operation::Purge) {
        return Ok(None);
    }
    let value = decode(key.as_str(), &entry.value)?;
    Ok(Some((value, Revision::new(entry.revision))))
}

pub(crate) async fn create_absent<V: Serialize>(
    kv: &Store,
    key: &KvKey,
    value: &V,
) -> Result<Revision, FabricError> {
    let bytes = encode(value)?;
    match kv.create(key.as_str(), bytes.into()).await {
        Ok(revision) => Ok(Revision::new(revision)),
        Err(err) => Err(map_create_error(key, err.kind(), &err)),
    }
}

pub(crate) async fn update_expecting<V: Serialize>(
    kv: &Store,
    key: &KvKey,
    value: &V,
    expected: Revision,
) -> Result<Revision, FabricError> {
    let bytes = encode(value)?;
    match kv.update(key.as_str(), bytes.into(), expected.get()).await {
        Ok(revision) => Ok(Revision::new(revision)),
        Err(err) => Err(map_revision_error(key, expected, err.kind(), &err)),
    }
}

pub(crate) async fn delete_expecting(
    kv: &Store,
    key: &KvKey,
    expected: Revision,
) -> Result<(), FabricError> {
    match kv
        .delete_expect_revision(key.as_str(), Some(expected.get()))
        .await
    {
        Ok(()) => Ok(()),
        Err(err) => Err(map_revision_error(key, expected, err.kind(), &err)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> KvKey {
        KvKey::new("identity/users/abc").unwrap()
    }

    #[test]
    fn wraps_and_exposes_the_sequence() {
        assert_eq!(Revision::new(7).get(), 7);
    }

    #[test]
    fn a_wrong_last_revision_maps_to_revision_conflict() {
        let err = map_revision_error(
            &key(),
            Revision::new(9),
            UpdateErrorKind::WrongLastRevision,
            "wrong last revision",
        );
        match err {
            FabricError::RevisionConflict { key, expected } => {
                assert_eq!(key, "identity/users/abc");
                assert_eq!(expected, 9);
            }
            other => panic!("expected RevisionConflict, got {other:?}"),
        }
    }

    #[test]
    fn other_revision_failures_stay_kv_errors() {
        for kind in [
            UpdateErrorKind::InvalidKey,
            UpdateErrorKind::TimedOut,
            UpdateErrorKind::Other,
        ] {
            let err = map_revision_error(&key(), Revision::new(9), kind, "boom");
            assert!(
                matches!(err, FabricError::Kv(detail) if detail == "boom"),
                "{kind:?} must not be reported as a revision conflict"
            );
        }
    }

    #[test]
    fn an_existing_key_maps_to_key_already_exists() {
        let err = map_create_error(&key(), CreateErrorKind::AlreadyExists, "already exists");
        match err {
            FabricError::KeyAlreadyExists { key } => assert_eq!(key, "identity/users/abc"),
            other => panic!("expected KeyAlreadyExists, got {other:?}"),
        }
    }

    #[test]
    fn other_create_failures_stay_kv_errors() {
        for kind in [
            CreateErrorKind::InvalidKey,
            CreateErrorKind::Publish,
            CreateErrorKind::Ack,
            CreateErrorKind::Other,
        ] {
            let err = map_create_error(&key(), kind, "boom");
            assert!(
                matches!(err, FabricError::Kv(detail) if detail == "boom"),
                "{kind:?} must not be reported as an existing key"
            );
        }
    }
}
