# Changelog — br-core-integration

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-05-22

Initial release. Provides:

- `MessageMetadata`, `IntegrationEvent<T>`, `IntegrationCommand<T>` envelopes.
- `IntegrationError` (publish / serialization).
- Object-safe `IntegrationPublisher` trait with `publish` and
  `publish_if_connected` methods.
- `IntegrationPublisherExt` blanket helpers for typed publishing
  (`publish_event`, `publish_command`, `_if_connected` variants).
- `NatsIntegrationPublisher` (JetStream, awaits broker ack on `publish`,
  logs and swallows errors on `publish_if_connected`).
- `NoopIntegrationPublisher` for tests.
