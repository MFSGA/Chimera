//! Pure construction of the portable lower-core reconcile intent.
//!
//! Runtime generation remains upstream for now; this service owns the portable
//! envelope half so facade/host routing does not derive wire identity itself.

use chimera_ipc::api::{core::v2::payload_digest, status::RevisionIdInfo};

use crate::{config::chimera::ClashCore, core::RunType};

use super::endpoint::{ConfigInput, CoreCommand, CoreSpec, ReconcileRequest};

pub(crate) struct RuntimeIntentBuilder;

impl RuntimeIntentBuilder {
    pub(crate) fn build(
        target_core: ClashCore,
        run_type: RunType,
        bytes: Vec<u8>,
        expected_applied: Option<RevisionIdInfo>,
    ) -> CoreCommand {
        let expected_digest = payload_digest(&bytes);
        CoreCommand::Reconcile(Box::new(ReconcileRequest {
            core: CoreSpec {
                target_core,
                run_type,
            },
            config: ConfigInput::Inline {
                bytes,
                expected_digest: Some(expected_digest),
            },
            expected_applied,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_inputs_produce_same_portable_intent() {
        let first = RuntimeIntentBuilder::build(
            ClashCore::Mihomo,
            RunType::Normal,
            b"mode: rule\n".to_vec(),
            None,
        );
        let second = RuntimeIntentBuilder::build(
            ClashCore::Mihomo,
            RunType::Normal,
            b"mode: rule\n".to_vec(),
            None,
        );

        let CoreCommand::Reconcile(second_request) = second else {
            panic!("intent builder must produce reconcile");
        };
        let CoreCommand::Reconcile(request) = first else {
            panic!("intent builder must produce reconcile");
        };
        let ConfigInput::Inline {
            bytes,
            expected_digest,
        } = &request.config;
        assert_eq!(bytes, b"mode: rule\n");
        assert_eq!(expected_digest, &Some(payload_digest(bytes)));
        assert_eq!(request.core.target_core, second_request.core.target_core);
        assert_eq!(request.core.run_type, second_request.core.run_type);
        assert_eq!(request.config, second_request.config);
    }

    #[test]
    fn config_change_changes_portable_identity() {
        let first = RuntimeIntentBuilder::build(
            ClashCore::Mihomo,
            RunType::Normal,
            b"mode: rule\n".to_vec(),
            None,
        );
        let second = RuntimeIntentBuilder::build(
            ClashCore::Mihomo,
            RunType::Normal,
            b"mode: global\n".to_vec(),
            None,
        );

        let CoreCommand::Reconcile(first) = first else {
            panic!("intent builder must produce reconcile");
        };
        let CoreCommand::Reconcile(second) = second else {
            panic!("intent builder must produce reconcile");
        };
        assert_ne!(first.config, second.config);
    }
}
