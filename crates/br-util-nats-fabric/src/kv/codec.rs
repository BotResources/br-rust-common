use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::FabricError;

pub(crate) fn encode<V: Serialize>(value: &V) -> Result<Vec<u8>, FabricError> {
    serde_json::to_vec(value).map_err(FabricError::from)
}

pub(crate) fn decode<V: DeserializeOwned>(key: &str, bytes: &[u8]) -> Result<V, FabricError> {
    serde_json::from_slice(bytes).map_err(|e| FabricError::decode(key, &e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
    struct Sample {
        name: String,
    }

    #[test]
    fn encode_then_decode_round_trips() {
        let value = Sample {
            name: "ada".to_string(),
        };
        let bytes = encode(&value).unwrap();
        let back: Sample = decode("identity/users/1", &bytes).unwrap();
        assert_eq!(back, value);
    }

    #[test]
    fn decode_fails_closed_and_names_the_key() {
        let err = decode::<Sample>("identity/users/1", b"{ not json").unwrap_err();
        match err {
            FabricError::Decode { subject, .. } => {
                assert_eq!(subject, "identity/users/1");
            }
            other => panic!("expected Decode, got {other:?}"),
        }
    }
}
