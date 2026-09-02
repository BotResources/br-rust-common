use crate::error::FabricError;
use crate::fabric::Fabric;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    Pending,
    Connected,
    Disconnected,
}

impl From<async_nats::connection::State> for ConnectionState {
    fn from(state: async_nats::connection::State) -> Self {
        use async_nats::connection::State as S;
        match state {
            S::Pending => Self::Pending,
            S::Connected => Self::Connected,
            S::Disconnected => Self::Disconnected,
        }
    }
}

#[derive(Clone)]
pub struct NatsAuth {
    pub user: String,
    pub password: String,
}

impl std::fmt::Debug for NatsAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatsAuth")
            .field("user", &self.user)
            .field("password", &"***")
            .finish()
    }
}

impl Fabric {
    pub async fn connect(url: &str) -> Result<Self, FabricError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| FabricError::connect(&e))?;
        Ok(Self::new(async_nats::jetstream::new(client)))
    }

    pub async fn connect_with(url: &str, auth: &NatsAuth) -> Result<Self, FabricError> {
        let client = async_nats::ConnectOptions::with_user_and_password(
            auth.user.clone(),
            auth.password.clone(),
        )
        .connect(url)
        .await
        .map_err(|e| FabricError::connect(&e))?;
        Ok(Self::new(async_nats::jetstream::new(client)))
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.context().client().connection_state().into()
    }

    pub fn reachable(&self) -> bool {
        self.connection_state() == ConnectionState::Connected
    }

    pub async fn ping(&self) -> Result<(), FabricError> {
        self.context()
            .client()
            .flush()
            .await
            .map_err(|e| FabricError::Connect(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nats_auth_debug_masks_the_password_and_never_prints_it() {
        let auth = NatsAuth {
            user: "fabric".to_string(),
            password: "s3cr3t-rotation-key".to_string(),
        };
        let rendered = format!("{auth:?}");
        assert!(
            !rendered.contains("s3cr3t-rotation-key"),
            "Debug leaked the password: {rendered}"
        );
        assert!(
            rendered.contains("***"),
            "Debug must mask the password with ***: {rendered}"
        );
        assert!(
            rendered.contains("fabric"),
            "Debug keeps the user for diagnostics: {rendered}"
        );
    }
}
