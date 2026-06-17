use async_nats::jetstream::kv::Store;

use crate::error::FabricError;
use crate::fabric::Fabric;

pub const KV_PUBLISHED_LANGUAGE: &str = "PUBLISHED_LANGUAGE";

impl Fabric {
    pub(crate) async fn published_language(&self) -> Result<Store, FabricError> {
        self.context()
            .get_key_value(KV_PUBLISHED_LANGUAGE)
            .await
            .map_err(FabricError::kv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bucket_constant_is_the_frozen_name() {
        assert_eq!(KV_PUBLISHED_LANGUAGE, "PUBLISHED_LANGUAGE");
    }
}
