use std::sync::Once;

use metrics::{Unit, counter, describe_counter};

pub const OUTBOX_RELAY_DUPLICATES_TOTAL: &str = "outbox_relay_duplicates_total";

static DESCRIBED: Once = Once::new();

pub(super) fn register() {
    DESCRIBED.call_once(|| {
        describe_counter!(
            OUTBOX_RELAY_DUPLICATES_TOTAL,
            Unit::Count,
            "Outbox rows whose publish the broker answered with a duplicate ack."
        );
    });
    counter!(OUTBOX_RELAY_DUPLICATES_TOTAL).increment(0);
}

pub(super) fn record_duplicate() {
    counter!(OUTBOX_RELAY_DUPLICATES_TOTAL).increment(1);
}
