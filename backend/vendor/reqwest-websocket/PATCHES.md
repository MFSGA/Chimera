# Local changes to reqwest-websocket 0.6.0

Source: the published 0.6.0 crate, licensed under MIT (see LICENSE).

Expose `NativeWebSocketStream` and `WebSocket::into_native_parts` on native
platforms. This additive API retains tungstenite text payloads without the
wrapper converting them to owned Strings. Upgrade validation, configuration,
protocol negotiation, transport handling and the existing message API are unchanged.

The Chimera application workspace patches crates.io to this directory so
`chimera-clash-api` can use the additive API. The separate runtime workspace
keeps its independently pinned transport client. Standalone consumers must
apply the same Cargo patch until an upstream API is available.
