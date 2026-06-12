use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedEntry<L> {
    pub locale: L,
    pub content: String,
}
