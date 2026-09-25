use std::fmt;
use std::str::FromStr;

use crate::error::InvalidValue;

/// The logical environment a service runs in: the value of `ENVIRONMENT`, and
/// the `botresources.ai/env` label the `br-common-service` chart sets beside it.
///
/// Deliberately exhaustive: a new environment must make every `match` on it —
/// typically a gate that allows a development-only backend in `Local` and
/// `Test` only — decide again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Environment {
    Local,
    Dev,
    Test,
    Uat,
    Prod,
}

impl Environment {
    pub const ALL: [Environment; 5] = [
        Environment::Local,
        Environment::Dev,
        Environment::Test,
        Environment::Uat,
        Environment::Prod,
    ];

    /// The one spelling `ENVIRONMENT` accepts: lowercase, as the chart's label.
    pub const fn as_str(self) -> &'static str {
        match self {
            Environment::Local => "local",
            Environment::Dev => "dev",
            Environment::Test => "test",
            Environment::Uat => "uat",
            Environment::Prod => "prod",
        }
    }
}

impl FromStr for Environment {
    type Err = InvalidValue;

    /// Exactly one of `local`, `dev`, `test`, `uat`, `prod` — no other case, no
    /// alias: the value is also a Kubernetes label, which is case-sensitive.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|environment| environment.as_str() == s)
            .ok_or_else(|| {
                InvalidValue::new(format!(
                    "unknown environment {s:?}: expected one of local, dev, test, uat, prod"
                ))
            })
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_environment_round_trips_through_its_spelling() {
        for environment in Environment::ALL {
            assert_eq!(environment.to_string().parse(), Ok(environment));
        }
    }

    #[test]
    fn spellings_are_the_lowercase_labels() {
        let spellings: Vec<_> = Environment::ALL.map(Environment::as_str).into();
        assert_eq!(spellings, ["local", "dev", "test", "uat", "prod"]);
    }

    #[test]
    fn another_case_or_an_alias_is_refused() {
        for refused in [
            "PROD",
            "Prod",
            "production",
            "development",
            "staging",
            " dev",
            "",
        ] {
            let err = refused.parse::<Environment>().unwrap_err();
            assert!(
                err.to_string().contains(&format!("{refused:?}")),
                "the refusal quotes the value: {err}"
            );
        }
    }
}
