use std::time::Duration;

use br_util_nats_fabric::{
    Aggregate, Bc, CommandCoords, ConsumeErrorKind, ConsumerTuning, EventCoords, Fabric,
    FabricError, INTEGRATION_CMD, INTEGRATION_EVT, PastFact, Verb,
};
use uuid::Uuid;

fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn fabric() -> Fabric {
    Fabric::new(jetstream().await)
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

fn user_created() -> EventCoords {
    EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    }
}

fn notification_deliver() -> CommandCoords {
    CommandCoords {
        receiver: Bc::new("notifier").unwrap(),
        aggregate: Aggregate::new("notification").unwrap(),
        verb: Verb::new("deliver").unwrap(),
        version: 1,
    }
}

async fn consumer_names(js: &async_nats::jetstream::Context, stream: &str) -> Vec<String> {
    use futures_util::TryStreamExt;
    let stream = js.get_stream(stream).await.expect("stream exists");
    stream
        .consumer_names()
        .try_collect()
        .await
        .expect("list consumer names")
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn verify_event_durable_passes_on_a_covering_stream_and_creates_no_consumer() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.>").await;

    let durable = format!("probe_{}", Uuid::now_v7().simple());
    fabric()
        .await
        .verify_event_durable(&user_created(), &durable)
        .await
        .expect("a live stream binding the coordinate verifies");

    let names = consumer_names(&js, INTEGRATION_EVT).await;
    assert!(
        !names.contains(&durable),
        "a readiness probe must not leave a phantom durable behind, found {names:?}"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn verify_event_durable_fails_loud_when_the_stream_is_absent() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let _ = js.delete_stream(INTEGRATION_EVT).await;

    let err = fabric()
        .await
        .verify_event_durable(&user_created(), "probe-absent-stream")
        .await
        .expect_err("an absent stream fails loud");

    assert!(
        matches!(
            err,
            FabricError::Consume {
                kind: ConsumeErrorKind::NoStream,
                ..
            }
        ),
        "expected NoStream, got {err:?}"
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn verify_event_durable_fails_when_the_stream_does_not_cover_the_coordinate() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.billing.>").await;

    let durable = format!("probe_{}", Uuid::now_v7().simple());
    let err = fabric()
        .await
        .verify_event_durable(&user_created(), &durable)
        .await
        .expect_err("a stream that does not bind the coordinate fails");

    match err {
        FabricError::SubjectNotCovered {
            stream,
            subject,
            configured,
        } => {
            assert_eq!(stream, INTEGRATION_EVT);
            assert_eq!(subject, "integration.evt.identity.user.created.v1");
            assert_eq!(configured, vec!["integration.evt.billing.>".to_string()]);
        }
        other => panic!("expected SubjectNotCovered, got {other:?}"),
    }

    assert!(
        !consumer_names(&js, INTEGRATION_EVT)
            .await
            .contains(&durable),
        "a failed probe must not leave a durable behind"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn ensure_event_durable_with_provisions_the_requested_ack_wait() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.>").await;

    let coords = user_created();
    let fabric = fabric().await;
    let stream = js.get_stream(INTEGRATION_EVT).await.unwrap();

    let tuned = format!("tuned_{}", Uuid::now_v7().simple());
    fabric
        .ensure_event_durable_with(
            &coords,
            &tuned,
            &ConsumerTuning {
                ack_wait: Duration::from_secs(2),
                max_ack_pending: 8,
            },
        )
        .await
        .expect("tuned provisioning");

    let info = stream.consumer_info(&tuned).await.expect("consumer info");
    assert_eq!(info.config.ack_wait, Duration::from_secs(2));
    assert_eq!(info.config.max_ack_pending, 8);
    assert_eq!(
        info.config.filter_subject,
        "integration.evt.identity.user.created.v1"
    );

    let defaulted = format!("default_{}", Uuid::now_v7().simple());
    fabric
        .ensure_event_durable(&coords, &defaulted)
        .await
        .expect("default provisioning");

    let info = stream
        .consumer_info(&defaulted)
        .await
        .expect("consumer info");
    assert_eq!(info.config.ack_wait, Duration::from_secs(30));
    assert_eq!(info.config.max_ack_pending, 256);

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn verify_command_durable_probes_the_command_stream_and_creates_no_consumer() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;
    let _ = js.delete_stream(INTEGRATION_EVT).await;

    let durable = format!("probe_{}", Uuid::now_v7().simple());
    fabric()
        .await
        .verify_command_durable(&notification_deliver(), &durable)
        .await
        .expect("the command probe reads INTEGRATION_CMD, not INTEGRATION_EVT");

    let names = consumer_names(&js, INTEGRATION_CMD).await;
    assert!(
        !names.contains(&durable),
        "a readiness probe must not leave a phantom durable behind, found {names:?}"
    );

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn verify_command_durable_fails_when_the_command_stream_does_not_cover_the_coordinate() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.billing.>").await;

    let err = fabric()
        .await
        .verify_command_durable(&notification_deliver(), "probe-uncovered-cmd")
        .await
        .expect_err("a command stream that does not bind the coordinate fails");

    match err {
        FabricError::SubjectNotCovered {
            stream,
            subject,
            configured,
        } => {
            assert_eq!(stream, INTEGRATION_CMD);
            assert_eq!(subject, "integration.cmd.notifier.notification.deliver.v1");
            assert_eq!(configured, vec!["integration.cmd.billing.>".to_string()]);
        }
        other => panic!("expected SubjectNotCovered, got {other:?}"),
    }

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn ensure_command_durable_with_provisions_on_the_command_stream() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;
    let _ = js.delete_stream(INTEGRATION_EVT).await;

    let durable = format!("tuned_{}", Uuid::now_v7().simple());
    fabric()
        .await
        .ensure_command_durable_with(
            &notification_deliver(),
            &durable,
            &ConsumerTuning {
                ack_wait: Duration::from_secs(2),
                max_ack_pending: 8,
            },
        )
        .await
        .expect("tuned provisioning on the command stream");

    let stream = js.get_stream(INTEGRATION_CMD).await.unwrap();
    let info = stream.consumer_info(&durable).await.expect("consumer info");
    assert_eq!(info.config.ack_wait, Duration::from_secs(2));
    assert_eq!(info.config.max_ack_pending, 8);
    assert_eq!(
        info.config.filter_subject,
        "integration.cmd.notifier.notification.deliver.v1"
    );

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}
