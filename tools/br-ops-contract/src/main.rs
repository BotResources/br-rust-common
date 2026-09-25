//! `br-ops-contract` — the ops-contract names the br-rust-common crates own,
//! as JSON: `{ "<crate>::<CONSTANT>": "<value>" }`, sorted by constant.
//!
//! The Helm library chart `br-common-service` renders these names (the boot
//! variables, the probe paths). A chart cannot import a Rust constant, so the
//! link is a gate in two hops, both run by CI on every pull request:
//!
//! 1. this crate's test fails when the committed
//!    `charts/br-common-service/ci/ops-contract.json` differs from what the
//!    constants print — regenerate it with
//!    `cargo run -q -p br-ops-contract > charts/br-common-service/ci/ops-contract.json`;
//! 2. `.github/scripts/check-chart.sh` fails when the chart renders a name or
//!    a default that differs from that file, renders a variable no constant
//!    owns (and the gate does not declare the chart's own), or leaves a
//!    constant unplaced.
//!
//! The committed file keeps the chart gate — and the release workflow that
//! re-runs it before publishing — free of a Rust toolchain.

use std::collections::BTreeMap;

/// `(the constant's path, its value)` for each constant, the path spelled by
/// the compiler from the very tokens that name the constant.
macro_rules! owned_names {
    ($($constant:path),+ $(,)?) => {
        [$((stringify!($constant), $constant)),+]
    };
}

fn contract() -> BTreeMap<&'static str, &'static str> {
    owned_names![
        br_util_boot::env::ENVIRONMENT,
        br_util_boot::env::PORT,
        br_util_boot::env::HOST,
        br_util_boot::env::DATABASE_URL,
        br_util_boot::env::NATS_URL,
        br_util_postgres::env::DATABASE_URL_OWNER,
        br_util_postgres::env::TRUSTED_NETWORK_HOSTS,
        br_util_observability::LIVENESS_PATH,
        br_util_observability::METRICS_PATH,
        br_util_axum_readiness::READINESS_PATH,
    ]
    .into_iter()
    .collect()
}

fn render() -> String {
    let json =
        serde_json::to_string_pretty(&contract()).expect("a map of strings always serializes");
    json + "\n"
}

fn main() {
    print!("{}", render());
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    const COMMITTED: &str = "charts/br-common-service/ci/ops-contract.json";

    #[test]
    fn the_committed_contract_is_what_the_constants_print() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(COMMITTED);
        let committed = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        assert_eq!(
            committed,
            render(),
            "{COMMITTED} is stale: regenerate it with \
             `cargo run -q -p br-ops-contract > {COMMITTED}`, then make the chart \
             render what it now says"
        );
    }

    #[test]
    fn every_name_is_a_variable_name_or_an_absolute_path() {
        for (constant, value) in contract() {
            let variable = !value.is_empty()
                && value
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                && !value.as_bytes()[0].is_ascii_digit();
            let path = value.starts_with('/');
            assert!(variable || path, "{constant} = {value:?}");
        }
    }

    #[test]
    fn keys_are_the_constants_paths() {
        assert_eq!(
            contract().get("br_util_boot::env::PORT"),
            Some(&br_util_boot::env::PORT)
        );
    }

    #[test]
    fn no_two_constants_own_the_same_name() {
        let names = contract();
        let mut values: Vec<_> = names.values().collect();
        values.sort();
        values.dedup();
        assert_eq!(values.len(), names.len());
    }
}
