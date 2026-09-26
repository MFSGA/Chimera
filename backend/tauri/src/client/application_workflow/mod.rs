//! Reference application-workflow classification modules migrated ahead of
//! the production workflow/coordinator that consumes them.

pub(crate) mod impact;
pub(in crate::client) mod inputs;
pub(crate) mod policy;
pub(in crate::client) mod profiles;
