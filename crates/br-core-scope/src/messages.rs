//! The three handshake payloads. A consumer composes each as the `T` of a
//! `br-core-integration` envelope — there is no dependency in this direction;
//! the envelope is generic over `T`, so these payloads stand alone.
//!
//! These are **payloads only**: correlation, causation, and timestamps live on
//! the envelope's `MessageMetadata` (in `br-core-integration`), so they are
//! deliberately absent here. A declaring service wraps [`DeclareServiceScopes`]
//! in an `IntegrationCommand<DeclareServiceScopes>` and publishes it on
//! `identity.cmd.service_scope.declare.v1`; Identity replies with an
//! `IntegrationEvent<ServiceScopesAccepted>` or
//! `IntegrationEvent<ServiceScopesRejected>` on the matching `.evt.` subject,
//! correlated by the envelope metadata.
//!
//! This crate carries no dependency on `br-core-integration`: the envelope is
//! generic over `T`, so the payloads stand alone and the two crates compose at
//! the consumer.

use serde::{Deserialize, Serialize};

use crate::declaration::ScopeDeclaration;
use crate::error::ScopeDeclarationError;
use crate::raw::RawScopeDeclaration;
use crate::service::ServiceKey;

/// Command payload: a service declares its manifest and scopes to Identity.
///
/// It carries the declaration in its **raw** ([`RawScopeDeclaration`]) form so
/// the receiver can always deserialize a structurally well-formed payload — even
/// one with a malformed key — and reply with a structured rejection rather than
/// nak/redeliver it forever (the handshake protocol forbids that loop; see
/// [`RawScopeDeclaration`]). The two sides bind to it asymmetrically:
///
/// - **Sender:** [`new`](Self::new) takes a *validated* [`ScopeDeclaration`], so
///   a well-behaved declarant can never put an invalid declaration on the wire.
/// - **Receiver:** deserialize, then call [`validate`](Self::validate) — the
///   explicit step that produces the structured
///   [`ScopeDeclarationError`] (including a genuinely-raised
///   [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey) on a malformed
///   key) used to build a [`ServiceScopesRejected`] reply.
///
/// The raw declaration is private: the only way to obtain a validated
/// [`ScopeDeclaration`] from a `DeclareServiceScopes` is [`validate`](Self::validate),
/// and the only way to build one is [`new`](Self::new) from a validated
/// declaration — the raw form is never mistakable for a validated one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareServiceScopes {
    /// The declaration being registered, in its raw (unvalidated) wire form.
    declaration: RawScopeDeclaration,
}

impl DeclareServiceScopes {
    /// Wrap a *validated* declaration as the declare-command payload — the
    /// sender's ergonomic path. The validated [`ScopeDeclaration`] is lowered to
    /// the raw wire form for transport; the wire JSON is identical to the raw
    /// form a receiver later validates.
    pub fn new(declaration: ScopeDeclaration) -> Self {
        Self {
            declaration: RawScopeDeclaration::from(declaration),
        }
    }

    /// The receiver's explicit validation step: validate the carried raw
    /// declaration into a [`ScopeDeclaration`], or return the single structured
    /// [`ScopeDeclarationError`] to put in a [`ServiceScopesRejected`] reply.
    ///
    /// # Errors
    ///
    /// Forwards every reason from [`RawScopeDeclaration::validate`]:
    /// [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey),
    /// [`ScopePrefixMismatch`](ScopeDeclarationError::ScopePrefixMismatch),
    /// [`DuplicateScopeInDeclaration`](ScopeDeclarationError::DuplicateScopeInDeclaration).
    pub fn validate(self) -> Result<ScopeDeclaration, ScopeDeclarationError> {
        self.declaration.validate()
    }

    /// Read-only access to the carried raw declaration (e.g. to log or inspect
    /// it before validating). Obtaining a validated [`ScopeDeclaration`] still
    /// requires [`validate`](Self::validate).
    pub fn raw(&self) -> &RawScopeDeclaration {
        &self.declaration
    }
}

/// Event payload: Identity accepted a service's declaration.
///
/// Carries the accepted service key so a subscriber can correlate on the
/// content as well as on the envelope metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceScopesAccepted {
    /// The service whose declaration was accepted.
    pub service: ServiceKey,
}

impl ServiceScopesAccepted {
    /// Build the accepted-event payload for `service`.
    pub fn new(service: ServiceKey) -> Self {
        Self { service }
    }
}

/// Event payload: Identity rejected a service's declaration.
///
/// Carries the service key and the **single** [`ScopeDeclarationError`] reason —
/// rejection is atomic, so one reason describes the whole refusal. The reason is
/// a stable code + params; the edge renders it per locale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceScopesRejected {
    /// The service whose declaration was rejected.
    pub service: ServiceKey,
    /// Why it was rejected (one reason; rejection is all-or-nothing).
    pub reason: ScopeDeclarationError,
}

impl ServiceScopesRejected {
    /// Build the rejected-event payload for `service` with `reason`.
    pub fn new(service: ServiceKey, reason: ScopeDeclarationError) -> Self {
        Self { service, reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KeyValidationError;
    use crate::key::ScopeKey;
    use crate::spec::{ScopeSpec, ServiceManifest};

    fn declaration() -> ScopeDeclaration {
        ScopeDeclaration::new(
            ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "l", "d"),
            vec![ScopeSpec::new(
                ScopeKey::new("notifier:read").unwrap(),
                "l",
                "d",
                false,
            )],
        )
        .unwrap()
    }

    #[test]
    fn declare_payload_roundtrip() {
        let cmd = DeclareServiceScopes::new(declaration());
        let json = serde_json::to_string(&cmd).unwrap();
        let back: DeclareServiceScopes = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    // The sender path (validated declaration → DeclareServiceScopes) and the
    // receiver path (deserialize → validate) exchange the SAME bytes: the wire
    // JSON of the declare payload is identical, and validating it on the receiver
    // side reproduces the very declaration the sender built.
    #[test]
    fn sender_and_receiver_paths_share_wire_and_declaration() {
        let decl = declaration();
        let cmd = DeclareServiceScopes::new(decl.clone());
        let json = serde_json::to_string(&cmd).unwrap();

        // Receiver deserializes the same bytes and validates.
        let received: DeclareServiceScopes = serde_json::from_str(&json).unwrap();
        assert_eq!(received.validate().unwrap(), decl);
    }

    // Receiver path on a malformed key: the payload is structurally well-formed
    // (it deserializes), and validate() yields the exact structured reason — never
    // an opaque serde error — so the receiver can reply ServiceScopesRejected.
    #[test]
    fn validate_yields_structured_reason_on_malformed_key() {
        let json = r#"{"declaration":{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:BAD","label_key":"l","description_key":"d","platform_only":false}]
        }}"#;
        let cmd: DeclareServiceScopes = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "notifier:BAD".to_string(),
                validation: KeyValidationError::InvalidCharset,
            }
        );
    }

    #[test]
    fn accepted_payload_roundtrip_and_shape() {
        let evt = ServiceScopesAccepted::new(ServiceKey::new("notifier").unwrap());
        let json = serde_json::to_value(&evt).unwrap();
        // The service key embeds as the bare string.
        assert_eq!(json["service"], "notifier");
        let back: ServiceScopesAccepted = serde_json::from_value(json).unwrap();
        assert_eq!(evt, back);
    }

    #[test]
    fn rejected_payload_roundtrip() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::ScopeOwnedByAnotherService {
                key: "notifier:read".to_string(),
                owner: "billing".to_string(),
            },
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: ServiceScopesRejected = serde_json::from_str(&json).unwrap();
        assert_eq!(evt, back);
    }

    // The rejected payload's exact JSON shape is load-bearing: it is the wire
    // contract a frontend / Identity binds to. Lock the nesting of the reason.
    #[test]
    fn rejected_payload_wire_shape() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "BAD".to_string(),
                validation: KeyValidationError::InvalidCharset,
            },
        );
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["service"], "notifier");
        // `ScopeDeclarationError` is internally tagged on `reason`; the
        // `InvalidScopeKey` variant flattens `key` + `validation` alongside it.
        assert_eq!(json["reason"]["reason"], "invalid_scope_key");
        assert_eq!(json["reason"]["key"], "BAD");
        // `KeyValidationError` is itself internally tagged on `validation`.
        assert_eq!(
            json["reason"]["validation"]["validation"],
            "invalid_charset"
        );
    }

    // The `ScopeOwnedByAnotherService` reason — produced only by Identity's
    // registry — must travel in a rejected payload like any other.
    #[test]
    fn rejected_payload_carries_registry_only_reason() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::ScopeOwnedByAnotherService {
                key: "notifier:read".to_string(),
                owner: "billing".to_string(),
            },
        );
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["reason"]["reason"], "scope_owned_by_another_service");
        assert_eq!(json["reason"]["owner"], "billing");
    }
}
