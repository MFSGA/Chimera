//! Bounded console collection shared by both embedded engines.
pub(crate) use chimera_profile_transform::{LogSpan, Logs};

pub(crate) const SCRIPT_LOG_ENTRY_LIMIT: usize = 256;
pub(crate) const SCRIPT_LOG_MESSAGE_LIMIT_BYTES: usize = 4 * 1024;
const SCRIPT_LOG_TRUNCATED_SUFFIX: &str = "… [truncated]";
const SCRIPT_LOG_LIMIT_WARNING: &str = "Additional transform logs were discarded.";

pub(crate) fn push_script_log(logs: &mut Logs, span: LogSpan, message: String) {
    if logs.len() >= SCRIPT_LOG_ENTRY_LIMIT {
        return;
    }
    if logs.len() == SCRIPT_LOG_ENTRY_LIMIT - 1 {
        logs.push((LogSpan::Warn, SCRIPT_LOG_LIMIT_WARNING.into()));
        return;
    }

    logs.push((span, truncate_script_log_message(message)));
}

fn truncate_script_log_message(mut message: String) -> String {
    if message.len() <= SCRIPT_LOG_MESSAGE_LIMIT_BYTES {
        return message;
    }

    let mut prefix_len =
        SCRIPT_LOG_MESSAGE_LIMIT_BYTES.saturating_sub(SCRIPT_LOG_TRUNCATED_SUFFIX.len());
    while prefix_len > 0 && !message.is_char_boundary(prefix_len) {
        prefix_len -= 1;
    }
    message.truncate(prefix_len);
    message.push_str(SCRIPT_LOG_TRUNCATED_SUFFIX);
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_logs_are_bounded_and_report_truncation() {
        let mut logs = Vec::new();
        for index in 0..SCRIPT_LOG_ENTRY_LIMIT + 20 {
            push_script_log(&mut logs, LogSpan::Info, format!("message-{index}"));
        }

        assert_eq!(logs.len(), SCRIPT_LOG_ENTRY_LIMIT);
        assert_eq!(
            logs.last(),
            Some(&(LogSpan::Warn, SCRIPT_LOG_LIMIT_WARNING.into()))
        );
    }

    #[test]
    fn script_log_messages_are_utf8_safely_truncated() {
        let mut logs = Vec::new();
        push_script_log(
            &mut logs,
            LogSpan::Log,
            "好".repeat(SCRIPT_LOG_MESSAGE_LIMIT_BYTES),
        );

        assert_eq!(logs.len(), 1);
        assert!(logs[0].1.len() <= SCRIPT_LOG_MESSAGE_LIMIT_BYTES);
        assert!(logs[0].1.ends_with("… [truncated]"));
    }
}
