# br-util-axum-auth

Axum middleware that extracts the `X-Passport` header into a request extension.

**Purpose.** `passport_header_middleware` decodes the base64-encoded JSON in
`X-Passport` and inserts the resulting `Passport` as a request extension.
Missing, empty, or malformed headers return `401 Unauthorized`.

**When to use.** An Axum-based service receives authenticated calls (via
`svc-identity` or a gateway) and wants the Passport available as an
`axum::Extension<Passport>` to handlers.

**When not to use.** The service uses a different HTTP framework, or does
its own identity extraction (e.g. parses a JWT directly).

**Current version.** `0.3.0`
