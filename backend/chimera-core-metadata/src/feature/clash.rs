//! Which Clash-family cores speak which optional protocol features, and from
//! which release onwards.
//!
//! The version floors here are the whole point of the module: a core kind alone
//! cannot answer "can I use this", because every capability below arrived in a
//! specific upstream release. Each floor is pinned to the commit that
//! introduced the feature and verified against the first tag containing it.

use std::sync::LazyLock;

use super::{CoreVersion, Support};
use enumset::{EnumSet, EnumSetType};
use schemars::JsonSchema;
use semver::VersionReq;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, EnumSetType, Type, JsonSchema, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    /// Supports a Windows named pipe for IPC.
    NamedPipeIpc,
    /// Supports a Unix domain socket for IPC.
    UnixSocketIpc,
    /// Supports running without a TCP external controller.
    DisableTcpController,
    /// Supports specifying a security descriptor when creating a Windows pipe.
    NamedPipeSecurityDescriptor,
}

pub trait FeatureSupport {
    /// Whether `self` speaks `feature`.
    ///
    /// A release version is compared after discarding its prerelease label.
    /// Nightly versions are treated as newer than every known floor, while an
    /// unknown version is denied. Pass `None` to retrieve an unresolved
    /// [`Support::Since`] requirement.
    fn supports(&self, feature: Feature, version: Option<&CoreVersion>) -> Support;

    /// Returns only features whose support is decided as [`Support::Yes`].
    fn features(&self, version: Option<&CoreVersion>) -> EnumSet<Feature> {
        EnumSet::all()
            .iter()
            .filter(|feature| matches!(self.supports(*feature, version), Support::Yes))
            .collect()
    }

    /// Returns features that may be supported by a sufficiently recent build.
    ///
    /// This is for display and probe short-circuiting only, never for enabling
    /// a capability.
    fn potential_features(&self) -> EnumSet<Feature> {
        EnumSet::all()
            .iter()
            .filter(|feature| !matches!(self.supports(*feature, None), Support::No))
            .collect()
    }
}

/// Mihomo grew the two controller transports a year apart:
/// `external-controller-unix` in v1.18.4 (commit `ca84ab1a`, absent in
/// v1.18.3), `external-controller-pipe` in v1.18.9 (commit `88bfe7cf`, absent
/// in v1.18.8).
///
/// Unlike clash-rs below, neither needs a later floor for WebSocket: both
/// listeners serve the same `router()` as the TCP controller over plain
/// HTTP/1.1, so `http.Hijacker` — and with it the `/traffic`, `/memory` and
/// `/logs` upgrades — works from the release that added each key.
static MIHOMO_UNIX: LazyLock<VersionReq> =
    LazyLock::new(|| VersionReq::parse(">=1.18.4").expect("valid Mihomo unix feature floor"));
static MIHOMO_PIPE: LazyLock<VersionReq> =
    LazyLock::new(|| VersionReq::parse(">=1.18.9").expect("valid Mihomo pipe feature floor"));

/// clash-rs added both keys at once in v0.9.1 (PR #867), but only the unix
/// listener was usable: it went through `axum::serve`, which always enables
/// upgrades, while the Windows named-pipe listener called
/// `hyper::server::conn::http1::Builder::serve_connection` *without*
/// `with_upgrades`. WebSocket endpoints could therefore never complete a
/// handshake over a named pipe until PR #1068 moved Windows onto `axum::serve`
/// too, first tagged in v0.9.7. That matters here because the `clash-api`
/// client drives `/traffic`, `/memory` and friends over the very same transport
/// it uses for REST, and clash-rs offers no newline-delimited-JSON fallback to
/// degrade to.
static CLASH_RS_UNIX: LazyLock<VersionReq> =
    LazyLock::new(|| VersionReq::parse(">=0.9.1").expect("valid clash-rs unix feature floor"));
static CLASH_RS_PIPE: LazyLock<VersionReq> =
    LazyLock::new(|| VersionReq::parse(">=0.9.7").expect("valid clash-rs pipe feature floor"));

/// `DisableTcpController` is exported capability metadata only and currently
/// gates no core-manager behavior; every core kind reports `Support::No`.
impl FeatureSupport for crate::kind::ClashCoreKind {
    fn supports(&self, feature: Feature, version: Option<&CoreVersion>) -> Support {
        match self {
            crate::kind::ClashCoreKind::Mihomo => match feature {
                Feature::NamedPipeIpc => since(&MIHOMO_PIPE, version),
                // v1.18.9 adapter/inbound/listen_windows.go reads LISTEN_NAMEDPIPE_SDDL.
                Feature::NamedPipeSecurityDescriptor => since(&MIHOMO_PIPE, version),
                Feature::UnixSocketIpc => since(&MIHOMO_UNIX, version),
                Feature::DisableTcpController => Support::No,
            },
            crate::kind::ClashCoreKind::ClashRust => match feature {
                Feature::NamedPipeIpc => since(&CLASH_RS_PIPE, version),
                Feature::NamedPipeSecurityDescriptor => Support::No,
                Feature::UnixSocketIpc => since(&CLASH_RS_UNIX, version),
                Feature::DisableTcpController => Support::No,
            },
            // Clash Premium only ever exposed `external-controller` over TCP.
            crate::kind::ClashCoreKind::ClashPremium => match feature {
                Feature::NamedPipeIpc
                | Feature::UnixSocketIpc
                | Feature::DisableTcpController
                | Feature::NamedPipeSecurityDescriptor => Support::No,
            },
            // meow-rs advertises `--ext-ctl-unix` and `--ext-ctl-pipe` in
            // `--help`, but only for mihomo CLI compatibility: both `bail!`
            // with "not yet supported" before startup (meow-app/src/main.rs,
            // v0.18.0 and current `main`). Passing either is therefore *worse*
            // than unsupported — it aborts the core instead of degrading, so
            // this must never resolve to `Yes` on the strength of the flag
            // existing. There is no YAML counterpart either: `raw.rs` has only
            // `external-controller`, parsed into a `SocketAddr`, and the repo
            // contains no `UnixListener` at all.
            crate::kind::ClashCoreKind::Meow => match feature {
                Feature::NamedPipeIpc
                | Feature::UnixSocketIpc
                | Feature::DisableTcpController
                | Feature::NamedPipeSecurityDescriptor => Support::No,
            },
        }
    }
}

/// Resolves a floor against a known version, or hands the requirement back when
/// the version is unknown.
fn since(req: &LazyLock<VersionReq>, version: Option<&CoreVersion>) -> Support {
    match version {
        Some(CoreVersion::Release(version)) if req.matches(version) => Support::Yes,
        Some(CoreVersion::Nightly) => Support::Yes,
        Some(CoreVersion::Release(_) | CoreVersion::Unknown) => Support::No,
        None => Support::Since((**req).clone()),
    }
}
