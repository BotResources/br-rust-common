# br-core-kernel

Minimal kernel types shared across BotResources Rust services.

**Purpose.** Typed ID wrappers that every service uses to refer to identities
without confusing a user ID with a service-account ID.

**When to use.** You need `UserId` or `ServiceAccountId` to pass or store an
actor identifier and want compile-time separation from other UUIDs.

**When not to use.** You need a generic UUID — use `uuid::Uuid` directly.
Don't add project-specific ID types here; keep them local to their bounded
context.

**Current version.** `0.1.0`
