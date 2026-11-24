#[derive(Debug)]
pub enum ConditionalReplaceResult<T> {
    Replaced(T),
    Conflict { actual_version: u64 },
}
