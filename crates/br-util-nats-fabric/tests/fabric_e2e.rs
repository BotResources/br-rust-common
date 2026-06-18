use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_core_integration::{EventMetadata, IntegrationCommand, IntegrationEvent, MessageOutcome};
use br_core_kernel::{Actor, UserId};
use br_util_nats_fabric::{
    Aggregate, Bc, CommandCoords, ConsumeErrorKind, EphemeralAuthStore, EventCoords, Fabric,
    FabricError, INTEGRATION_CMD, INTEGRATION_EVT, KV_EPHEMERAL_AUTH, KV_PUBLISHED_LANGUAGE, KvKey,
    KvPrefix, PastFact, ProjectionSink, PublishedLanguageConsumer, PublishedLanguagePublisher,
    PublishedLanguageReader, Revision, Verb,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Payload {
    label: String,
}

fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

async fn fabric() -> Fabric {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    Fabric::new(async_nats::jetstream::new(client))
}

async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn recreate_stream(js: &async_nats::jetstream::Context, name: &str, bind: &str) {
    let _ = js.delete_stream(name).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name: name.to_string(),
        subjects: vec![bind.to_string()],
        ..Default::default()
    })
    .await
    .expect("create fixed stream");
}

fn command(label: &str, correlation_id: Uuid) -> IntegrationCommand<Payload> {
    IntegrationCommand::new(
        Uuid::now_v7(),
        "notification.deliver",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id),
        Payload {
            label: label.to_string(),
        },
    )
}

fn event(label: &str, correlation_id: Uuid) -> IntegrationEvent<Payload> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "user.created",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id),
        Payload {
            label: label.to_string(),
        },
    )
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn command_renders_grammar_and_a_matching_durable_consumes_it() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let coords = CommandCoords {
        receiver: Bc::new("notifier").unwrap(),
        aggregate: Aggregate::new("notification").unwrap(),
        verb: Verb::new("deliver").unwrap(),
        version: 1,
    };
    let durable = format!("test_{}", Uuid::now_v7().simple());
    let stream = js.get_stream(INTEGRATION_CMD).await.unwrap();
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(durable.clone()),
            filter_subject: "integration.cmd.notifier.notification.deliver.v1".to_string(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await
        .unwrap();

    let fabric = fabric().await;
    let correlation = Uuid::now_v7();
    fabric
        .publish_command(&coords, &command("hello", correlation))
        .await
        .expect("publish command");

    let (seen_tx, seen_rx) = tokio::sync::oneshot::channel::<String>();
    let seen_tx = std::sync::Arc::new(std::sync::Mutex::new(Some(seen_tx)));
    let consumer = tokio::spawn(async move {
        fabric
            .run_commands::<Payload, _, _, _>(
                &coords,
                &durable,
                move |delivery| {
                    let seen_tx = seen_tx.clone();
                    async move {
                        if let Some(tx) = seen_tx.lock().unwrap().take() {
                            let _ = tx.send(delivery.envelope.payload.label.clone());
                        }
                        MessageOutcome::Ack
                    }
                },
                |_| {},
            )
            .await
    });

    let label = tokio::time::timeout(Duration::from_secs(5), seen_rx)
        .await
        .expect("durable consumed the command within the deadline")
        .expect("handler signalled the payload");
    assert_eq!(label, "hello");

    consumer.abort();
    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn a_widened_durable_is_rejected_on_bind() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.>").await;

    let durable = format!("wide_{}", Uuid::now_v7().simple());
    let stream = js.get_stream(INTEGRATION_EVT).await.unwrap();
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(durable.clone()),
            filter_subject: "integration.evt.>".to_string(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await
        .unwrap();

    let coords = EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    };
    let fabric = fabric().await;
    let err = fabric
        .verify_event_durable(&coords, &durable)
        .await
        .unwrap_err();
    assert!(matches!(err, FabricError::FilterMismatch { .. }));

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_matches_by_correlation_id() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.>").await;

    let coords = EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    };
    let fabric = fabric().await;
    let mut awaiter = fabric.await_event(&coords).await.expect("await_event");

    let correlation = Uuid::now_v7();
    fabric
        .publish_event(&coords, &event("evt", correlation))
        .await
        .expect("publish event");

    let matched = awaiter
        .await_correlation(correlation, Duration::from_secs(5))
        .await
        .expect("await_correlation");
    assert!(matched.is_some());

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn published_language_binds_existing_bucket_and_fails_loud_when_absent() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let _ = js.delete_key_value(KV_PUBLISHED_LANGUAGE).await;

    let fabric = fabric().await;
    let absent = PublishedLanguagePublisher::<Payload>::open(&fabric).await;
    assert!(matches!(absent, Err(FabricError::Kv(_))));

    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_PUBLISHED_LANGUAGE.to_string(),
        ..Default::default()
    })
    .await
    .expect("create bucket");
    assert!(
        PublishedLanguagePublisher::<Payload>::open(&fabric)
            .await
            .is_ok()
    );

    let _ = js.delete_key_value(KV_PUBLISHED_LANGUAGE).await;
}

async fn ensure_published_language_bucket(
    js: &async_nats::jetstream::Context,
) -> async_nats::jetstream::kv::Store {
    if let Ok(store) = js.get_key_value(KV_PUBLISHED_LANGUAGE).await {
        return store;
    }
    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_PUBLISHED_LANGUAGE.to_string(),
        ..Default::default()
    })
    .await
    .expect("create bucket")
}

fn isolated_key(suffix: &str) -> String {
    format!("plget/{}/{suffix}", Uuid::now_v7().simple())
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn single_key_get_returns_none_for_an_absent_key() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let _ = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");
    let key = KvKey::new(isolated_key("absent")).unwrap();
    assert_eq!(reader.get(&key).await.expect("get"), None);
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn single_key_get_returns_the_decoded_value_for_a_present_key() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let store = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let key = KvKey::new(isolated_key("present")).unwrap();
    let value = Payload {
        label: "manifest".to_string(),
    };
    store
        .put(key.as_str(), serde_json::to_vec(&value).unwrap().into())
        .await
        .expect("put");

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");
    assert_eq!(reader.get(&key).await.expect("get"), Some(value));
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn single_key_get_fails_closed_on_an_undecodable_value() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let store = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let key = KvKey::new(isolated_key("garbage")).unwrap();
    store
        .put(key.as_str(), b"{ not json".to_vec().into())
        .await
        .expect("put");

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");
    match reader.get(&key).await {
        Err(FabricError::Decode { subject, .. }) => assert_eq!(subject, key.as_str()),
        other => panic!("expected Decode naming the key, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn single_key_get_is_exact_and_does_not_match_a_prefix_sibling() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let store = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let base = isolated_key("meta");
    let key = KvKey::new(base.clone()).unwrap();
    let sibling = KvKey::new(format!("{base}data")).unwrap();
    let base_value = Payload {
        label: "meta".to_string(),
    };
    let sibling_value = Payload {
        label: "sibling".to_string(),
    };
    store
        .put(
            key.as_str(),
            serde_json::to_vec(&base_value).unwrap().into(),
        )
        .await
        .expect("put base");
    store
        .put(
            sibling.as_str(),
            serde_json::to_vec(&sibling_value).unwrap().into(),
        )
        .await
        .expect("put sibling");

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");
    assert_eq!(reader.get(&key).await.expect("get"), Some(base_value));
    assert_eq!(
        reader.get(&sibling).await.expect("get"),
        Some(sibling_value)
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn enumeration_returns_only_the_prefix_scoped_keys_and_entries() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let store = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let run = Uuid::now_v7().simple().to_string();
    let prefix = KvPrefix::new(format!("plenum/{run}/users/")).unwrap();
    let inside = [
        (
            KvKey::new(format!("plenum/{run}/users/ada")).unwrap(),
            Payload {
                label: "ada".to_string(),
            },
        ),
        (
            KvKey::new(format!("plenum/{run}/users/grace")).unwrap(),
            Payload {
                label: "grace".to_string(),
            },
        ),
    ];
    let outside = (
        KvKey::new(format!("plenum/{run}/groups/admins")).unwrap(),
        Payload {
            label: "admins".to_string(),
        },
    );
    for (key, value) in inside.iter().chain(std::iter::once(&outside)) {
        store
            .put(key.as_str(), serde_json::to_vec(value).unwrap().into())
            .await
            .expect("put");
    }

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");

    let keys = reader.keys(&prefix).await.expect("keys");
    assert_eq!(
        keys,
        vec![inside[0].0.clone(), inside[1].0.clone()],
        "keys must be exactly the prefix-scoped set, sorted"
    );

    let entries = reader.entries(&prefix).await.expect("entries");
    let expected: BTreeMap<KvKey, Payload> = inside.iter().cloned().collect();
    assert_eq!(entries, expected);
    assert!(
        !entries.contains_key(&outside.0),
        "an entry outside the prefix must never be enumerated"
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn enumeration_entries_fails_closed_on_an_undecodable_value() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let store = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let run = Uuid::now_v7().simple().to_string();
    let prefix = KvPrefix::new(format!("plenum/{run}/users/")).unwrap();
    let good = KvKey::new(format!("plenum/{run}/users/ada")).unwrap();
    let bad = KvKey::new(format!("plenum/{run}/users/garbage")).unwrap();
    store
        .put(
            good.as_str(),
            serde_json::to_vec(&Payload {
                label: "ada".to_string(),
            })
            .unwrap()
            .into(),
        )
        .await
        .expect("put good");
    store
        .put(bad.as_str(), b"{ not json".to_vec().into())
        .await
        .expect("put bad");

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");
    match reader.entries(&prefix).await {
        Err(FabricError::Decode { subject, .. }) => assert_eq!(subject, bad.as_str()),
        other => panic!("expected Decode naming the bad key, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn enumeration_on_an_unmatched_prefix_returns_a_legitimate_empty() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let run = Uuid::now_v7().simple().to_string();
    let prefix = KvPrefix::new(format!("plenum/{run}/never-written/")).unwrap();

    let reader = PublishedLanguageReader::<Payload>::open(&fabric)
        .await
        .expect("open reader");

    assert_eq!(reader.keys(&prefix).await.expect("keys"), Vec::new());
    assert_eq!(
        reader.entries(&prefix).await.expect("entries"),
        BTreeMap::new()
    );
}

#[derive(Clone, Default)]
struct RecordingSink {
    projected: Arc<Mutex<BTreeMap<KvKey, Payload>>>,
}

#[async_trait::async_trait]
impl ProjectionSink<Payload> for RecordingSink {
    type Error = std::convert::Infallible;

    async fn project(&self, key: &KvKey, value: &Payload) -> Result<(), Self::Error> {
        self.projected
            .lock()
            .unwrap()
            .insert(key.clone(), value.clone());
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        self.projected.lock().unwrap().remove(key);
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        Ok(self.projected.lock().unwrap().keys().cloned().collect())
    }
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn watch_delivers_a_live_slash_keyed_directory_put() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let store = ensure_published_language_bucket(&js).await;
    let fabric = fabric().await;

    let sink = RecordingSink::default();
    let projected = sink.projected.clone();
    let consumer = PublishedLanguageConsumer::<Payload, _, _>::open(
        &fabric,
        vec![KvPrefix::new("identity/users/").unwrap()],
        |_: &Payload| true,
        sink,
    )
    .await
    .expect("open consumer");
    let watcher = tokio::spawn(async move {
        let _ = consumer.watch().await;
    });

    let id = Uuid::now_v7();
    let key = format!("identity/users/{id}");
    let kvkey = KvKey::new(key.clone()).unwrap();
    let value = Payload {
        label: "live".to_string(),
    };
    let body = serde_json::to_vec(&value).unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
    loop {
        store.put(&key, body.clone().into()).await.expect("put");
        let delivered = tokio::time::timeout(Duration::from_millis(400), async {
            loop {
                if projected.lock().unwrap().contains_key(&kvkey) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .is_ok();
        if delivered {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "live watch never delivered the slash-keyed put {key} — KvPrefix::watch_subject regression (#82)"
        );
    }

    assert_eq!(projected.lock().unwrap().get(&kvkey), Some(&value));

    watcher.abort();
    let _ = store.purge(&key).await;
}

async fn ensure_ephemeral_auth_bucket(
    js: &async_nats::jetstream::Context,
) -> async_nats::jetstream::kv::Store {
    if let Ok(store) = js.get_key_value(KV_EPHEMERAL_AUTH).await {
        return store;
    }
    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_EPHEMERAL_AUTH.to_string(),
        history: 8,
        max_age: Duration::from_secs(3600),
        ..Default::default()
    })
    .await
    .expect("create ephemeral-auth bucket")
}

fn ephemeral_key(suffix: &str) -> KvKey {
    KvKey::new(format!("auth/refresh/{}/{suffix}", Uuid::now_v7().simple())).unwrap()
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn ephemeral_auth_binds_existing_bucket_and_fails_loud_when_absent() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;

    let fabric = fabric().await;
    let absent = EphemeralAuthStore::<Payload>::open(&fabric).await;
    assert!(matches!(absent, Err(FabricError::Kv(_))));

    ensure_ephemeral_auth_bucket(&js).await;
    assert!(EphemeralAuthStore::<Payload>::open(&fabric).await.is_ok());

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn concurrent_update_if_on_the_same_revision_has_exactly_one_winner() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ephemeral_auth_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let key = ephemeral_key("family");

    store
        .create(
            &key,
            &Payload {
                label: "seed".to_string(),
            },
        )
        .await
        .expect("seed create");

    let (_, rev) = store
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("present");

    let value_a = Payload {
        label: "rotation-a".to_string(),
    };
    let value_b = Payload {
        label: "rotation-b".to_string(),
    };
    let a = store.update_if(&key, &value_a, rev);
    let b = store.update_if(&key, &value_b, rev);
    let (ra, rb) = tokio::join!(a, b);

    let outcomes = [ra, rb];
    let winners = outcomes.iter().filter(|r| r.is_ok()).count();
    let conflicts = outcomes
        .iter()
        .filter(|r| matches!(r, Err(FabricError::RevisionConflict { .. })))
        .count();
    assert_eq!(winners, 1, "exactly one writer wins on the same revision");
    assert_eq!(conflicts, 1, "the loser gets the distinguishable conflict");

    let _ = store
        .put(
            &key,
            &Payload {
                label: "wipe".into(),
            },
        )
        .await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn put_performs_the_unconditional_revoke_family_wipe() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ephemeral_auth_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let key = ephemeral_key("revoke");

    store
        .create(
            &key,
            &Payload {
                label: "active".to_string(),
            },
        )
        .await
        .expect("create");

    store
        .put(
            &key,
            &Payload {
                label: "revoked".to_string(),
            },
        )
        .await
        .expect("unconditional put ignores the revision chain");

    let (value, _) = store
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(value.label, "revoked");
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn create_re_creates_through_a_post_delete_tombstone_where_absent_update_if_conflicts() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let raw = ensure_ephemeral_auth_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let key = ephemeral_key("recreate");

    store
        .create(
            &key,
            &Payload {
                label: "first-life".to_string(),
            },
        )
        .await
        .expect("first create");

    raw.delete(key.as_str())
        .await
        .expect("delete leaves a tombstone at seq>0");

    assert_eq!(
        store.get_with_revision(&key).await.expect("get"),
        None,
        "a deleted key reads as absent"
    );

    let conflict = store
        .update_if(
            &key,
            &Payload {
                label: "via-absent-update".to_string(),
            },
            Revision::ABSENT,
        )
        .await;
    assert!(
        matches!(conflict, Err(FabricError::RevisionConflict { .. })),
        "update_if(ABSENT) conflicts forever against the tombstone — this is why create() is needed: {conflict:?}"
    );

    store
        .create(
            &key,
            &Payload {
                label: "second-life".to_string(),
            },
        )
        .await
        .expect("create re-creates through the tombstone");

    let (value, _) = store
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("present");
    assert_eq!(value.label, "second-life");

    let _ = raw.purge(key.as_str()).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn create_on_a_live_key_returns_key_already_exists() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ephemeral_auth_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let key = ephemeral_key("live");

    store
        .create(
            &key,
            &Payload {
                label: "occupant".to_string(),
            },
        )
        .await
        .expect("first create");

    let again = store
        .create(
            &key,
            &Payload {
                label: "intruder".to_string(),
            },
        )
        .await;
    assert!(
        matches!(again, Err(FabricError::KeyAlreadyExists { .. })),
        "create on a live key is a distinguishable conflict: {again:?}"
    );

    let _ = store
        .put(
            &key,
            &Payload {
                label: "wipe".into(),
            },
        )
        .await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn get_with_revision_fails_closed_on_an_undecodable_value() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let raw = ensure_ephemeral_auth_bucket(&js).await;
    let fabric = fabric().await;

    let key = ephemeral_key("garbage");
    raw.put(key.as_str(), b"{ not json".to_vec().into())
        .await
        .expect("put garbage");

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    match store.get_with_revision(&key).await {
        Err(FabricError::Decode { subject, .. }) => assert_eq!(subject, key.as_str()),
        other => panic!("expected Decode naming the key, got {other:?}"),
    }
}

const DELIVER_FILTER: &str = "integration.cmd.notifier.notification.deliver.v1";

fn deliver_coords() -> CommandCoords {
    CommandCoords {
        receiver: Bc::new("notifier").unwrap(),
        aggregate: Aggregate::new("notification").unwrap(),
        verb: Verb::new("deliver").unwrap(),
        version: 1,
    }
}

async fn create_deliver_durable(
    js: &async_nats::jetstream::Context,
    durable: &str,
    max_deliver: i64,
    ack_wait: Duration,
) {
    let stream = js.get_stream(INTEGRATION_CMD).await.unwrap();
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(durable.to_string()),
            filter_subject: DELIVER_FILTER.to_string(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            max_deliver,
            ack_wait,
            ..Default::default()
        })
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn bind_command_consumer_acks_and_the_message_is_not_redelivered() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let durable = format!("ack_{}", Uuid::now_v7().simple());
    create_deliver_durable(&js, &durable, 5, Duration::from_secs(1)).await;

    let coords = deliver_coords();
    let fabric = fabric().await;
    fabric
        .publish_command(&coords, &command("hello", Uuid::now_v7()))
        .await
        .expect("publish command");

    let mut consumer = fabric
        .bind_command_consumer::<Payload>(&coords, &durable)
        .await
        .expect("bind durable");

    let delivery = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("recv within deadline")
        .expect("recv ok")
        .expect("a delivery");
    assert_eq!(delivery.payload().unwrap().payload.label, "hello");
    assert_eq!(delivery.delivered_count(), 1);
    delivery.ack().await.expect("ack");

    let after_ack = tokio::time::timeout(Duration::from_secs(2), consumer.recv()).await;
    assert!(
        after_ack.is_err(),
        "an acked message must not be redelivered"
    );

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn nak_redelivers_after_the_delay_and_delivered_count_increments() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let durable = format!("nak_{}", Uuid::now_v7().simple());
    create_deliver_durable(&js, &durable, 5, Duration::from_secs(30)).await;

    let coords = deliver_coords();
    let fabric = fabric().await;
    fabric
        .publish_command(&coords, &command("again", Uuid::now_v7()))
        .await
        .expect("publish command");

    let mut consumer = fabric
        .bind_command_consumer::<Payload>(&coords, &durable)
        .await
        .expect("bind durable");

    let first = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("first recv within deadline")
        .expect("recv ok")
        .expect("a delivery");
    assert_eq!(first.delivered_count(), 1);
    first
        .nak(Some(Duration::from_millis(500)))
        .await
        .expect("nak");

    let second = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("redelivered within deadline")
        .expect("recv ok")
        .expect("a redelivery");
    assert_eq!(
        second.delivered_count(),
        2,
        "delivered_count increments on redelivery"
    );
    second.ack().await.expect("ack");

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn a_poison_frame_is_routable_to_term_and_the_loop_survives() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let durable = format!("poison_{}", Uuid::now_v7().simple());
    create_deliver_durable(&js, &durable, 5, Duration::from_secs(1)).await;

    let coords = deliver_coords();
    let fabric = fabric().await;

    js.publish(DELIVER_FILTER.to_string(), b"{ not json".to_vec().into())
        .await
        .expect("publish poison")
        .await
        .expect("poison ack");
    fabric
        .publish_command(&coords, &command("good", Uuid::now_v7()))
        .await
        .expect("publish good command");

    let mut consumer = fabric
        .bind_command_consumer::<Payload>(&coords, &durable)
        .await
        .expect("bind durable");

    let poison = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("poison recv within deadline")
        .expect("recv ok")
        .expect("a delivery");
    match poison.payload() {
        Err(FabricError::Decode { subject, .. }) => assert_eq!(subject, DELIVER_FILTER),
        other => panic!("expected a routable Decode error, got {other:?}"),
    }
    poison.term().await.expect("term the poison frame");

    let good = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("loop survives and yields the next frame")
        .expect("recv ok")
        .expect("a delivery");
    assert_eq!(good.payload().unwrap().payload.label, "good");
    good.ack().await.expect("ack");

    let after_term = tokio::time::timeout(Duration::from_secs(2), consumer.recv()).await;
    assert!(
        after_term.is_err(),
        "a termed poison frame must not be redelivered"
    );

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn delivered_count_increments_across_redeliveries_up_to_the_budget() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let durable = format!("budget_{}", Uuid::now_v7().simple());
    create_deliver_durable(&js, &durable, 3, Duration::from_secs(30)).await;

    let coords = deliver_coords();
    let fabric = fabric().await;
    fabric
        .publish_command(&coords, &command("retry", Uuid::now_v7()))
        .await
        .expect("publish command");

    let mut consumer = fabric
        .bind_command_consumer::<Payload>(&coords, &durable)
        .await
        .expect("bind durable");

    for expected in 1..=3 {
        let delivery = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
            .await
            .expect("recv within deadline")
            .expect("recv ok")
            .expect("a delivery");
        assert_eq!(
            delivery.delivered_count(),
            expected,
            "delivered_count tracks the attempt number"
        );
        delivery
            .nak(Some(Duration::from_millis(200)))
            .await
            .expect("nak");
    }

    let exhausted = tokio::time::timeout(Duration::from_secs(2), consumer.recv()).await;
    assert!(
        exhausted.is_err(),
        "no further delivery once max_deliver is exhausted"
    );

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn bind_command_consumer_fails_loud_when_the_durable_is_absent() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let coords = deliver_coords();
    let fabric = fabric().await;
    let absent = format!("missing_{}", Uuid::now_v7().simple());
    match fabric
        .bind_command_consumer::<Payload>(&coords, &absent)
        .await
    {
        Err(FabricError::Consume {
            kind: ConsumeErrorKind::NoConsumer,
            ..
        }) => {}
        Err(other) => panic!("expected NoConsumer, got {other:?}"),
        Ok(_) => panic!("binding an absent durable must fail loud, never auto-create"),
    }

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}
