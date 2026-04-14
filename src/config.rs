use std::fmt;
use std::str::FromStr;

use crate::error::InfraError;

// ---------------------------------------------------------------------------
// Environment
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Local,
    Dev,
    Test,
    Prod,
}

impl FromStr for Environment {
    type Err = InfraError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "dev" | "development" => Ok(Self::Dev),
            "test" => Ok(Self::Test),
            "prod" | "production" => Ok(Self::Prod),
            other => Err(InfraError::Config(format!("unknown environment: {other}"))),
        }
    }
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Local => "local",
            Self::Dev => "dev",
            Self::Test => "test",
            Self::Prod => "prod",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Config {
    pub environment: Environment,
    pub database_url: String,
    pub host: String,
    pub port: u16,
    pub allow_insecure: bool,
}

impl Config {
    /// Load configuration from environment variables.
    pub fn from_env() -> Result<Self, InfraError> {
        let _ = dotenvy::dotenv();

        let environment: Environment = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "local".to_string())
            .parse()?;

        let database_url = std::env::var("DATABASE_URL")
            .map_err(|_| InfraError::Config("DATABASE_URL is required".to_string()))?;

        let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        let port: u16 = std::env::var("PORT")
            .map_err(|_| InfraError::Config("PORT is required".to_string()))?
            .parse()
            .map_err(|e| InfraError::Config(format!("invalid PORT: {e}")))?;

        let allow_insecure: bool = std::env::var("ALLOW_INSECURE")
            .unwrap_or_else(|_| "false".to_string())
            .parse()
            .map_err(|e| InfraError::Config(format!("invalid ALLOW_INSECURE: {e}")))?;

        if allow_insecure && environment == Environment::Prod {
            return Err(InfraError::Config(
                "ALLOW_INSECURE=true is forbidden in production".to_string(),
            ));
        }

        Ok(Self {
            environment,
            database_url,
            host,
            port,
            allow_insecure,
        })
    }

    pub fn is_prod(&self) -> bool {
        self.environment == Environment::Prod
    }

    pub fn is_local(&self) -> bool {
        self.environment == Environment::Local
    }

    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_from_str_case_insensitive() {
        assert_eq!("local".parse::<Environment>().unwrap(), Environment::Local);
        assert_eq!("LOCAL".parse::<Environment>().unwrap(), Environment::Local);
        assert_eq!("dev".parse::<Environment>().unwrap(), Environment::Dev);
        assert_eq!(
            "development".parse::<Environment>().unwrap(),
            Environment::Dev
        );
        assert_eq!("test".parse::<Environment>().unwrap(), Environment::Test);
        assert_eq!("prod".parse::<Environment>().unwrap(), Environment::Prod);
        assert_eq!(
            "production".parse::<Environment>().unwrap(),
            Environment::Prod
        );
        assert!("unknown".parse::<Environment>().is_err());
    }

    #[test]
    fn environment_display_lowercase() {
        assert_eq!(Environment::Local.to_string(), "local");
        assert_eq!(Environment::Dev.to_string(), "dev");
        assert_eq!(Environment::Test.to_string(), "test");
        assert_eq!(Environment::Prod.to_string(), "prod");
    }
}
