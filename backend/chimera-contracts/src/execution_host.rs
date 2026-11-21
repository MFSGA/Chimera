//! The execution host identity shared by public results.

/// Which controller owns the runtime. The app perceives the difference in
/// exactly two places: this tag on the endpoint slot, and the handoff
/// protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionHost {
    Local,
    Service,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_host_wire_distinguishes_local_and_service() {
        assert_eq!(serde_json::to_value(ExecutionHost::Local).unwrap(), "local");
        assert_eq!(
            serde_json::to_value(ExecutionHost::Service).unwrap(),
            "service"
        );
        assert_eq!(
            serde_json::from_str::<ExecutionHost>("\"service\"").unwrap(),
            ExecutionHost::Service
        );
    }
}
