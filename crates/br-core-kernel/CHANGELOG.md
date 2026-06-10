# Changelog — br-core-kernel

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.4.0] — 2026-06-10

**Changed (BREAKING)**
- Removed `Deref<Target = Uuid>` from `UserId` and `ServiceAccountId`. Deref
  coercion silently turned a typed id back into a `&Uuid` anywhere a `&Uuid`
  was expected (UUID-keyed maps, SQL binds, `&Uuid`-taking functions),
  reopening the "two UUIDs are interchangeable" hole these newtypes exist to
  close. *Migration:* replace any `&*id` or implicit `&Uuid` coercion with
  `id.as_uuid()` (by value) or `id.as_ref()` (by reference). Wire format is
  unchanged (see below); only Rust call sites that relied on deref are
  affected.

**Added**
- `pub const fn as_uuid(&self) -> Uuid` on both types — explicit access to the
  inner `Uuid` by value (`Uuid` is `Copy`).
- `impl AsRef<Uuid>` on both types — explicit access by reference.
- `impl From<UserId> for Uuid` / `impl From<ServiceAccountId> for Uuid` — the
  outbound counterpart of the existing `From<Uuid>`, for call sites that take
  `impl Into<Uuid>`. (`FromStr` is deliberately not provided: parse the `Uuid`,
  then wrap — `UserId::from(s.parse::<Uuid>()?)` keeps the two steps explicit.)
- `#[serde(transparent)]` on both types. The JSON wire format (a plain UUID
  string) is **unchanged** from 0.3.x — it is byte-for-byte identical to
  serde's default newtype encoding for these single-field tuple structs — but
  it is now a declared, tested contract rather than an incidental default.

**Fixed**
- The README claimed `repr(Uuid)` (not valid Rust; implies a layout guarantee
  that did not exist) and described the serde encoding as "transparent" while
  the code carried a plain `#[derive(Serialize, Deserialize)]` with no
  `#[serde(transparent)]`. Both are now accurate: the README says "thin
  newtype around `Uuid`", and the transparent wire format is enforced by the
  attribute and proven by tests.

## [0.3.1] — 2026-05-22

**Changed**
- Workspace metadata cleanup: `edition`, `rust-version`, `license`, and
  `repository` now inherit from `[workspace.package]` via
  `.workspace = true`. The crate's `rust-version` was previously declared as
  `1.85` per-crate while the workspace, CI, and top-level README all
  advertised `1.88`; the inherited value is now consistently `1.88`. No API
  or runtime behavior change.

## [0.3.0] — 2026-04-17

- Carved out of `br-service-core` during the workspace split into
  `br-rust-common`. Provides typed ID wrappers (`UserId`,
  `ServiceAccountId`).
