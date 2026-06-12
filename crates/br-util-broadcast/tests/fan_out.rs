//! Behavioural specs for the in-process event bus, Given/When/Then.
//!
//! Exercises the public surface exactly as a consumer would: fan-out to one and
//! many subscribers, the lagged / closed receiver behaviour, the no-subscribers
//! signal, channel sharing across clones, and the load-bearing **post-commit
//! ordering contract** — that a `PendingBroadcast` reaches no subscriber until
//! it is handed to `publish_after_commit`.

use br_util_broadcast::{BroadcastError, EventBus, PendingBroadcast};
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

/// The bus is generic; a `String` payload stands in for `DomainEvent` so the
/// test stays domain-free, like the crate.
fn bus() -> EventBus<String> {
    EventBus::new(16)
}

fn pending(events: &[&str]) -> PendingBroadcast<String> {
    PendingBroadcast::from_events(events.iter().map(|s| (*s).to_string()).collect())
}

#[tokio::test]
async fn given_one_subscriber_when_published_after_commit_then_it_receives_in_order() {
    // Given a subscriber on the bus
    let bus = bus();
    let mut rx = bus.subscribe();

    // When two events are published after commit
    bus.publish_after_commit(pending(&["A", "B"]))
        .expect("a subscriber is listening");

    // Then the subscriber receives both, in publish order
    assert_eq!(rx.recv().await.unwrap(), "A");
    assert_eq!(rx.recv().await.unwrap(), "B");
}

#[tokio::test]
async fn given_many_subscribers_when_published_then_each_receives_the_same_event() {
    // Given two independent subscribers
    let bus = bus();
    let mut rx1 = bus.subscribe();
    let mut rx2 = bus.subscribe();
    assert_eq!(bus.subscriber_count(), 2);

    // When one event is published after commit
    bus.publish_after_commit(pending(&["fan-out"])).unwrap();

    // Then both subscribers receive it (multicast)
    assert_eq!(rx1.recv().await.unwrap(), "fan-out");
    assert_eq!(rx2.recv().await.unwrap(), "fan-out");
}

#[tokio::test]
async fn given_no_subscriber_when_events_published_then_no_subscribers_signal() {
    // Given a bus with nobody listening
    let bus = bus();
    assert_eq!(bus.subscriber_count(), 0);

    // When events are published after commit
    let outcome = bus.publish_after_commit(pending(&["X", "Y"]));

    // Then the informational no-subscribers signal carries the unheard count —
    // not a write failure: the events are already committed and durable.
    assert_eq!(outcome, Err(BroadcastError::NoSubscribers { unheard: 2 }));
}

#[tokio::test]
async fn given_an_empty_buffer_when_published_then_it_is_a_legal_no_op() {
    // Given no subscriber and an empty buffer (a command that changed no state)
    let bus = bus();

    // When the empty buffer is published after commit
    let outcome = bus.publish_after_commit(PendingBroadcast::<String>::new());

    // Then it is a no-op success, never a no-subscribers error
    assert!(outcome.is_ok());
}

#[tokio::test]
async fn given_a_closed_receiver_when_others_listen_then_fan_out_still_succeeds() {
    // Given two subscribers, one of which is then dropped (closed)
    let bus = bus();
    let rx_closed = bus.subscribe();
    let mut rx_open = bus.subscribe();
    drop(rx_closed);
    assert_eq!(bus.subscriber_count(), 1);

    // When an event is published
    bus.publish_after_commit(pending(&["survives"])).unwrap();

    // Then the surviving subscriber still receives it
    assert_eq!(rx_open.recv().await.unwrap(), "survives");
}

#[tokio::test]
async fn given_all_receivers_closed_when_published_then_no_subscribers_signal() {
    // Given a subscriber that is then dropped, leaving nobody
    let bus = bus();
    let rx = bus.subscribe();
    drop(rx);

    // When an event is published
    let outcome = bus.publish_after_commit(pending(&["unheard"]));

    // Then the no-subscribers signal is raised
    assert_eq!(outcome, Err(BroadcastError::NoSubscribers { unheard: 1 }));
}

#[tokio::test]
async fn given_a_slow_subscriber_when_it_overflows_capacity_then_it_lags() {
    // Given a small-capacity bus and a subscriber that never drains
    let bus: EventBus<u64> = EventBus::new(2);
    let mut rx = bus.subscribe();

    // When more events are published than the buffer holds
    for n in 0..5_u64 {
        bus.publish_after_commit(PendingBroadcast::from_events(vec![n]))
            .unwrap();
    }

    // Then the receiver is told it lagged (lost the oldest events) and then
    // resumes from the retained tail — recovery is a reconnect/replay concern,
    // never a crash.
    match rx.recv().await {
        Err(RecvError::Lagged(skipped)) => assert!(skipped >= 1),
        other => panic!("expected Lagged, got {other:?}"),
    }
    // The next reads are the retained tail (last `capacity` events).
    let next = rx.recv().await.unwrap();
    assert!(next >= 3);
}

#[tokio::test]
async fn given_a_cloned_bus_when_it_publishes_then_subscribers_of_the_original_receive() {
    // Given a subscriber on the original bus and a clone of that bus
    let bus = bus();
    let mut rx = bus.subscribe();
    let cloned = bus.clone();

    // When the clone publishes after commit
    cloned
        .publish_after_commit(pending(&["shared-channel"]))
        .unwrap();

    // Then the original's subscriber receives it — clone shares the channel
    assert_eq!(rx.recv().await.unwrap(), "shared-channel");
}

/// The load-bearing spec: a buffer built *before* the commit step reaches no
/// subscriber until it is handed to `publish_after_commit`. There is no API to
/// fan a lone event out mid-transaction — the buffer carries no channel.
#[tokio::test]
async fn given_a_pending_buffer_built_pre_commit_then_nothing_is_delivered_until_after_commit() {
    // Given a subscriber and a buffer accumulated while the "command" runs
    let bus = bus();
    let mut rx = bus.subscribe();

    let mut pending = PendingBroadcast::new();
    pending.push("evt-1".to_string());
    pending.extend(["evt-2".to_string(), "evt-3".to_string()]);
    assert_eq!(pending.len(), 3);

    // When we have NOT yet called publish_after_commit (stand-in for "the tx is
    // still open"), the subscriber has received nothing — the buffer holds no
    // channel, so there is no way for it to have leaked an event.
    assert_eq!(rx.try_recv(), Err(TryRecvError::Empty));

    // When the "commit" boundary is crossed and we publish after commit
    bus.publish_after_commit(pending).unwrap();

    // Then, and only then, all staged events are delivered in order
    assert_eq!(rx.recv().await.unwrap(), "evt-1");
    assert_eq!(rx.recv().await.unwrap(), "evt-2");
    assert_eq!(rx.recv().await.unwrap(), "evt-3");
}

#[tokio::test]
async fn given_events_collected_via_from_iter_then_they_fan_out_in_order() {
    // Given a buffer built from an iterator of domain-command output
    let bus = bus();
    let mut rx = bus.subscribe();
    let pending: PendingBroadcast<String> = ["one", "two"].into_iter().map(String::from).collect();

    // When published after commit
    bus.publish_after_commit(pending).unwrap();

    // Then the events arrive in iteration order
    assert_eq!(rx.recv().await.unwrap(), "one");
    assert_eq!(rx.recv().await.unwrap(), "two");
}
