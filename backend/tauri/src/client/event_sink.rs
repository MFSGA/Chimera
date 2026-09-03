//! UI event boundary for application client side effects.

use crate::core::handle::Handle;

pub(crate) trait UiEventSink: Send + Sync {
    fn refresh_clash(&self);
    fn refresh_runtime_transform_diagnostics(&self);
    fn refresh_profiles(&self);
}

pub(crate) struct LegacyUiEventSink;

impl UiEventSink for LegacyUiEventSink {
    fn refresh_clash(&self) {
        Handle::refresh_clash();
    }

    fn refresh_runtime_transform_diagnostics(&self) {
        Handle::refresh_runtime_transform_diagnostics();
    }

    fn refresh_profiles(&self) {
        Handle::refresh_profiles();
    }
}
