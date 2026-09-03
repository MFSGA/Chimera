#[derive(Debug)]
pub(crate) enum ConditionalReplaceResult<T> {
    Replaced(T),
    Conflict { actual_version: u64 },
}

pub mod application;
pub mod mirror;
