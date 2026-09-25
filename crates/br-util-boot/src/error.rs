use std::fmt;

use thiserror::Error;

/// Why a value was refused. Operator-facing copy: it quotes the value of a
/// plain variable (`PORT`, `ENVIRONMENT`) so that a typo is visible, and never
/// the value of a variable that carries a credential (`DATABASE_URL`,
/// `NATS_URL`).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct InvalidValue(String);

impl InvalidValue {
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self(reason.into())
    }
}

/// One variable of the boot environment that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum VarProblem {
    #[error("{var} is not set")]
    Missing { var: &'static str },

    #[error("{var} is set but empty")]
    Empty { var: &'static str },

    #[error("{var} is not valid UTF-8")]
    NotUnicode { var: &'static str },

    #[error("{var} is invalid: {reason}")]
    Invalid {
        var: &'static str,
        reason: InvalidValue,
    },
}

impl VarProblem {
    /// The name of the variable, one of the constants of [`crate::env`].
    pub fn var(&self) -> &'static str {
        match self {
            Self::Missing { var }
            | Self::Empty { var }
            | Self::NotUnicode { var }
            | Self::Invalid { var, .. } => var,
        }
    }
}

/// The boot environment cannot be read: every problem found, in the order the
/// variables are read, so an operator fixes them in one pass. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootEnvError {
    problems: Vec<VarProblem>,
}

impl BootEnvError {
    pub(crate) fn new(problems: Vec<VarProblem>) -> Self {
        debug_assert!(
            !problems.is_empty(),
            "a BootEnvError names at least one problem"
        );
        Self { problems }
    }

    pub fn problems(&self) -> &[VarProblem] {
        &self.problems
    }
}

impl fmt::Display for BootEnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid boot environment: ")?;
        for (i, problem) in self.problems.iter().enumerate() {
            if i > 0 {
                f.write_str("; ")?;
            }
            write!(f, "{problem}")?;
        }
        Ok(())
    }
}

impl std::error::Error for BootEnvError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_lists_every_problem_in_order() {
        let err = BootEnvError::new(vec![
            VarProblem::Missing { var: "PORT" },
            VarProblem::Invalid {
                var: "ENVIRONMENT",
                reason: InvalidValue::new("unknown environment \"staging\""),
            },
        ]);
        assert_eq!(
            err.to_string(),
            "invalid boot environment: PORT is not set; \
             ENVIRONMENT is invalid: unknown environment \"staging\""
        );
    }

    #[test]
    fn every_problem_names_its_variable() {
        let problems = [
            VarProblem::Missing { var: "A" },
            VarProblem::Empty { var: "B" },
            VarProblem::NotUnicode { var: "C" },
            VarProblem::Invalid {
                var: "D",
                reason: InvalidValue::new("x"),
            },
        ];
        let vars: Vec<_> = problems.iter().map(VarProblem::var).collect();
        assert_eq!(vars, ["A", "B", "C", "D"]);
    }
}
