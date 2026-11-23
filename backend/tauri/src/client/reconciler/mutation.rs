//! Completed reconciliation rounds, following ref/client/reconciler/mutation.rs.
//! No second runtime mutator is introduced: this is the one actor's history.
use std::sync::Arc;

use chimera_core::client::reconciler::plan::{Conclusion, RuntimeError};

#[derive(Debug, Clone)]
pub(crate) struct RoundRecord {
    pub operation_id: u64,
    pub trigger: String,
    pub conclusion: String,
    pub cause: Option<Arc<RuntimeError>>,
}

impl RoundRecord {
    pub(super) fn new(operation_id: u64, trigger: &str, conclusion: &Conclusion) -> Self {
        let (label, cause) = match conclusion {
            Conclusion::Converged => ("Converged", None),
            Conclusion::Inactive => ("Inactive", None),
            Conclusion::NotInEffect { cause } => ("NotInEffect", Some(cause.clone())),
            Conclusion::Waiting { cause } => ("Waiting", Some(cause.clone())),
            Conclusion::Unknown { cause } => ("Unknown", Some(cause.clone())),
            Conclusion::Closed => ("Closed", None),
        };
        Self {
            operation_id,
            trigger: trigger.to_owned(),
            conclusion: label.to_owned(),
            cause,
        }
    }
}
