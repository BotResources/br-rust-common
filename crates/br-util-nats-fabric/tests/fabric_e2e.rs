use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_core_integration::{EventMetadata, IntegrationCommand, IntegrationEvent, MessageOutcome};
use br_core_kernel::{Actor, UserId};
use br_util_nats_fabric::{
    Aggregate, Bc, CommandCoords, EventCoords, Fabric, FabricError, INTEGRATION_CMD,
    INTEGRATION_EVT, KV_PUBLISHED_LANGUAGE, KvKey, KvPrefix, PastFact, ProjectionSink,
    PublishedLanguageConsumer, PublishedLanguagePublisher, PublishedLanguageReader, Verb,
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
