#[derive(Debug)]
pub(crate) enum ConditionalReplaceResult<T> {
    Replaced(T),
    Conflict { actual_version: u64 },
}

pub mod application;
pub mod clash_config;
pub mod mirror;
