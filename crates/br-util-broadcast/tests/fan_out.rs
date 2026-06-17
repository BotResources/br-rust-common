use br_util_broadcast::{BroadcastError, EventBus, PendingBroadcast};
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

fn bus() -> EventBus<String> {
    EventBus::new(16)
}

fn pending(events: &[&str]) -> PendingBroadcast<String> {
    PendingBroadcast::from_events(events.iter().map(|s| (*s).to_string()).collect())
}

#[tokio::test]
async fn given_one_subscriber_when_published_after_commit_then_it_receives_in_order() {
    let bus = bus();
    let mut rx = bus.subscribe();

    bus.publish_after_commit(pending(&["A", "B"]))
        .expect("a subscriber is listening");

    assert_eq!(rx.recv().await.unwrap(), "A");
    assert_eq!(rx.recv().await.unwrap(), "B");
}

#[tokio::test]
async fn given_many_subscribers_when_published_then_each_receives_the_same_event() {
    let bus = bus();
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);

    bus.publish_after_commit(pending(&["fan-out"])).unwrap();

    assert_eq!(rx1.recv().await.unwrap(), "fan-out");
    assert_eq!(rx2.recv().await.unwrap(), "fan-out");
}

#[tokio::test]
async fn given_no_subscriber_when_events_published_then_no_subscribers_signal() {
    let bus = bus();
    assert_eq!(bus.subscriber_count(), 0);

    let outcome = bus.publish_after_commit(pending(&["X", "Y"]));

    assert_eq!(outcome, Err(BroadcastError::NoSubscribers { unheard: 2 }));
}

#[tokio::test]
async fn given_an_empty_buffer_when_published_then_it_is_a_legal_no_op() {
    let bus = bus();

    let outcome = bus.publish_after_commit(PendingBroadcast::<String>::new());

    assert!(outcome.is_ok());
}

#[tokio::test]
async fn given_a_closed_receiver_when_others_listen_then_fan_out_still_succeeds() {
    let bus = bus();
    let rx_closed = bus.subscribe();
    let mut rx_open = bus.subscribe();
    drop(rx_closed);
    assert_eq!(bus.subscriber_count(), 1);

    bus.publish_after_commit(pending(&["survives"])).unwrap();

    assert_eq!(rx_open.recv().await.unwrap(), "survives");
}

#[tokio::test]
async fn given_all_receivers_closed_when_published_then_no_subscribers_signal() {
    let bus = bus();
    let rx = bus.subscribe();
    drop(rx);

    let outcome = bus.publish_after_commit(pending(&["unheard"]));

    assert_eq!(outcome, Err(BroadcastError::NoSubscribers { unheard: 1 }));
}

#[test]
#[should_panic(expected = "EventBus capacity must be > 0")]
fn given_zero_capacity_when_constructed_then_it_panics_with_a_precise_message() {
    let _bus: EventBus<String> = EventBus::new(0);
}

#[tokio::test]
async fn given_a_slow_subscriber_when_it_overflows_capacity_then_it_lags() {
    let bus: EventBus<u64> = EventBus::new(2);
    let mut rx = bus.subscribe();

    for n in 0..5_u64 {
        bus.publish_after_commit(PendingBroadcast::from_events(vec![n]))
            .unwrap();
    }

    match rx.recv().await {
        Err(RecvError::Lagged(skipped)) => assert!(skipped >= 1),
        other => panic!("expected Lagged, got {other:?}"),
    }
    let next = rx.recv().await.unwrap();
    assert!(next >= 3);
}

#[tokio::test]
async fn given_a_cloned_bus_when_it_publishes_then_subscribers_of_the_original_receive() {
    let bus = bus();
    let mut rx = bus.subscribe();
    let cloned = bus.clone();

    cloned
        .publish_after_commit(pending(&["shared-channel"]))
        .unwrap();

    assert_eq!(rx.recv().await.unwrap(), "shared-channel");
}

#[tokio::test]
async fn given_a_pending_buffer_built_pre_commit_then_nothing_is_delivered_until_after_commit() {
    let bus = bus();
    let mut rx = bus.subscribe();

    let mut pending = PendingBroadcast::new();
    pending.push("evt-1".to_string());
    pending.extend(["evt-2".to_string(), "evt-3".to_string()]);
    assert_eq!(pending.len(), 3);

    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

    bus.publish_after_commit(pending).unwrap();

    assert_eq!(rx.recv().await.unwrap(), "evt-1");
    assert_eq!(rx.recv().await.unwrap(), "evt-2");
    assert_eq!(rx.recv().await.unwrap(), "evt-3");
}

#[tokio::test]
async fn given_events_collected_via_from_iter_then_they_fan_out_in_order() {
    let bus = bus();
    let mut rx = bus.subscribe();
    let pending: PendingBroadcast<String> = ["one", "two"].into_iter().map(String::from).collect();

    bus.publish_after_commit(pending).unwrap();

    assert_eq!(rx.recv().await.unwrap(), "one");
    assert_eq!(rx.recv().await.unwrap(), "two");
}
