# br-core-auth

Passport identity DTO and `X-Passport` header codec.

**Purpose.** `Passport` (Human | Service) is the identity representation that
`svc-identity` builds and every downstream service consumes. `PassportHeader`
encapsulates the base64/JSON encoding for the `X-Passport` HTTP header.

**When to use.** Any service that authenticates incoming requests (receives
`X-Passport`) or propagates identity downstream.

**When not to use.** You are inside a bounded context that has already
extracted the identity from the Passport into its own domain types. Don't
pass `Passport` through the domain layer.

**Current version.** `0.1.0`
