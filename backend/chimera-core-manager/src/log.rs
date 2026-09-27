//! Normalization of core console output into a single [`LogFrame`] shape.
//!
//! Each kind prints its own layout (mihomo logfmt, clash premium's `PrettyPrint`,
//! and two different tracing formats), all of them on stdout, and only meow keeps
//! ANSI when writing to a pipe. Parsing is header-only and per kind: a line whose
//! header does not match degrades to an unformatted frame instead of being lost.

use std::borrow::Borrow;

use chrono::{DateTime, Duration, FixedOffset, NaiveDate, NaiveTime, TimeZone};

pub use chimera_core_metadata::{LogField, LogFrame, LogLevel, LogStream, LogTimestamp};

use crate::kind::CoreKind;

pub(crate) const LOG_CHANNEL_CAPACITY: usize = 256;
const MAX_CONTINUATION_LINES: usize = 16;

/// The caps every frame leaving this parser respects. They live here, and only
/// here, because bounding at the source is what lets every downstream consumer —
/// the 32-frame diagnostic tail, the JSONL archive, the ws stream — take the
/// frame as it is. A frame that arrives anywhere else has already been cut.
const MAX_LOG_TEXT_BYTES: usize = 16 * 1024;
const MAX_LOG_TARGET_BYTES: usize = 2048;
const MAX_LOG_TIMESTAMP_RAW_BYTES: usize = 256;
const MAX_LOG_FIELDS: usize = 64;
const MAX_LOG_FIELD_TEXT_BYTES: usize = 1024;

/// At most two frames leave the parser per line: a flushed multi-line record and
/// the line that ended it.
pub(crate) type ParsedFrames = [Option<LogFrame>; 2];

struct ParsedLine {
    timestamp: Option<LogTimestamp>,
    level: LogLevel,
    target: Option<String>,
    message: String,
    fields: Vec<LogField>,
}

struct Pending {
    frame: LogFrame,
    continuations: usize,
}

impl Pending {
    fn append(&mut self, line: &str) {
        if self.continuations == MAX_CONTINUATION_LINES {
            self.frame.truncated = true;
            return;
        }
        // Never short-circuits: both texts take the same line, and they run out
        // of budget at different points because `raw` also carries the header.
        let cut = append_bounded(&mut self.frame.raw, line, MAX_LOG_TEXT_BYTES)
            | append_bounded(&mut self.frame.message, line, MAX_LOG_TEXT_BYTES);
        if cut {
            self.frame.truncated = true;
            // One of the two texts is now at its cap, so there is no room for a
            // further continuation either.
            self.continuations = MAX_CONTINUATION_LINES;
        } else {
            self.continuations += 1;
        }
    }
}

/// Appends `\n` and as much of `line` as fits. `true` means something was left
/// behind — the caller stops appending rather than growing a hole.
///
/// The separator comes out of the same budget as the content, and it is only
/// written once something can follow it: a line whose first character does not
/// fit is dropped whole rather than turned into a blank one.
fn append_bounded(text: &mut String, line: &str, max_bytes: usize) -> bool {
    if text.len() >= max_bytes {
        return true;
    }
    // An empty continuation *is* the separator, so it always fits here.
    if line.is_empty() {
        text.push('\n');
        return false;
    }
    let budget = max_bytes - text.len() - 1;
    let end = char_boundary_at_or_below(line, budget.min(line.len()));
    if end == 0 {
        return true;
    }
    text.push('\n');
    text.push_str(&line[..end]);
    end < line.len()
}

/// Cuts `text` down to `max_bytes`. `true` means it was cut.
fn truncate_text(text: &mut String, max_bytes: usize) -> bool {
    if text.len() <= max_bytes {
        return false;
    }
    text.truncate(char_boundary_at_or_below(text, max_bytes));
    true
}

/// The largest index at or below `max_bytes` that does not split a character,
/// so a cut string is still a `str` and still serializes as valid JSON.
fn char_boundary_at_or_below(text: &str, max_bytes: usize) -> usize {
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

/// Brings every free-text field within its cap, in place.
///
/// Runs exactly once per frame, before it is emitted or held. Everything
/// downstream — the diagnostic tail, the JSONL archive, the ws stream — then
/// consumes the frame as it stands, which is why none of them clamp any more.
fn bound_frame(frame: &mut LogFrame) {
    let mut cut = truncate_text(&mut frame.message, MAX_LOG_TEXT_BYTES);
    cut |= truncate_text(&mut frame.raw, MAX_LOG_TEXT_BYTES);
    if let Some(target) = frame.target.as_mut() {
        cut |= truncate_text(target, MAX_LOG_TARGET_BYTES);
    }
    if let Some(timestamp) = frame.timestamp.as_mut() {
        cut |= truncate_text(&mut timestamp.raw, MAX_LOG_TIMESTAMP_RAW_BYTES);
    }
    if frame.fields.len() > MAX_LOG_FIELDS {
        frame.fields.truncate(MAX_LOG_FIELDS);
        cut = true;
    }
    for field in &mut frame.fields {
        cut |= truncate_text(&mut field.key, MAX_LOG_FIELD_TEXT_BYTES);
        cut |= truncate_text(&mut field.value, MAX_LOG_FIELD_TEXT_BYTES);
    }
    frame.truncated |= cut;
}

/// Stateful because clash premium needs the previous timestamp to infer date
/// rollovers, and because a fatal record can span several lines.
pub(crate) struct LogParser {
    kind: CoreKind,
    epoch: u64,
    premium_clock: Option<(NaiveDate, NaiveTime)>,
    pending: [Option<Pending>; 2],
    newest_pending: Option<LogStream>,
}

impl LogParser {
    pub(crate) fn new(kind: CoreKind, epoch: u64) -> Self {
        Self {
            kind,
            epoch,
            premium_clock: None,
            pending: [None, None],
            newest_pending: None,
        }
    }

    pub(crate) fn push(&mut self, stream: LogStream, line: String) -> ParsedFrames {
        self.push_at(stream, line, chrono::Local::now().fixed_offset())
    }

    pub(crate) fn push_at(
        &mut self,
        stream: LogStream,
        line: String,
        observed_at: DateTime<FixedOffset>,
    ) -> ParsedFrames {
        let line = strip_ansi(line);
        if let Some(parsed) = self.parse_header(&line, observed_at) {
            let hold = parsed.level >= LogLevel::Error;
            let mut frame = LogFrame {
                at: observed_at.timestamp_millis(),
                epoch: self.epoch,
                kind: self.kind,
                stream,
                level: parsed.level,
                timestamp: parsed.timestamp,
                target: parsed.target,
                message: parsed.message,
                fields: parsed.fields,
                raw: line,
                truncated: false,
            };
            bound_frame(&mut frame);
            return self.emit(stream, frame, hold);
        }

        // clash-rs and meow also write plain, level-less text to stderr.
        let error_root = line.starts_with("Error:");
        let warning_root = line.starts_with("warning:");
        if !error_root
            && !warning_root
            && let Some(pending) = self.pending[stream_index(stream)].as_mut()
        {
            pending.append(&line);
            return [None, None];
        }
        let level = match (error_root, warning_root, stream) {
            (true, _, _) => LogLevel::Error,
            (_, true, _) => LogLevel::Warning,
            (.., LogStream::Stdout) => LogLevel::Info,
            (.., LogStream::Stderr) => LogLevel::Warning,
        };
        let mut frame = LogFrame {
            at: observed_at.timestamp_millis(),
            epoch: self.epoch,
            kind: self.kind,
            stream,
            level,
            timestamp: None,
            target: None,
            message: line.clone(),
            fields: Vec::new(),
            raw: line,
            truncated: false,
        };
        bound_frame(&mut frame);
        self.emit(stream, frame, error_root && stream == LogStream::Stderr)
    }

    /// Releases records still waiting for continuations, for process termination
    /// and for buffered one-shot output.
    pub(crate) fn finish(&mut self) -> ParsedFrames {
        let stdout = self.pending[0].take().map(|pending| pending.frame);
        let stderr = self.pending[1].take().map(|pending| pending.frame);
        // Oldest first, so consumers that read the tail backwards still see the
        // newest record first when both streams held one.
        match self.newest_pending.take() {
            Some(LogStream::Stdout) => [stderr, stdout],
            _ => [stdout, stderr],
        }
    }

    fn parse_header(
        &mut self,
        line: &str,
        observed_at: DateTime<FixedOffset>,
    ) -> Option<ParsedLine> {
        match self.kind {
            CoreKind::Mihomo => parse_mihomo(line),
            CoreKind::Meow => parse_meow(line),
            CoreKind::ClashRust => parse_clash_rs(line, observed_at),
            CoreKind::ClashPremium => parse_premium(line, observed_at, &mut self.premium_clock),
        }
    }

    /// Continuations only ever attach to a root from the same stream, so an
    /// interleaved stdout line cannot be glued onto a stderr error block.
    fn emit(&mut self, stream: LogStream, frame: LogFrame, hold: bool) -> ParsedFrames {
        let index = stream_index(stream);
        let flushed = self.pending[index].take().map(|pending| pending.frame);
        if hold {
            self.newest_pending = Some(stream);
            self.pending[index] = Some(Pending {
                frame,
                continuations: 0,
            });
            [flushed, None]
        } else {
            [flushed, Some(frame)]
        }
    }
}

/// The most severe recent frame, latest first within that severity. `None` when
/// nothing above `Info` was logged.
///
/// Generic over the borrow so the owned frames a one-shot run produces and the
/// `Arc`-shared frames the diagnostic tail holds both go straight in.
pub(crate) fn error_summary<T: Borrow<LogFrame>>(frames: &[T]) -> Option<String> {
    let level = frames
        .iter()
        .map(|frame| Borrow::<LogFrame>::borrow(frame).level)
        .filter(|level| *level >= LogLevel::Warning)
        .max()?;
    frames
        .iter()
        .rev()
        .map(Borrow::<LogFrame>::borrow)
        .find(|frame| frame.level == level)
        .map(|frame| frame.message.clone())
}

pub(crate) fn format_tail<T: Borrow<LogFrame>>(frames: &[T]) -> String {
    frames
        .iter()
        .map(|frame| Borrow::<LogFrame>::borrow(frame).raw.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The two streams a one-shot run produced. They are both `&str`, so a
/// transposed call parses stderr as stdout and reports the wrong stream's
/// last error as the cause of a failed config check.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CapturedOutput<'a> {
    pub stdout: &'a str,
    pub stderr: &'a str,
}

/// Condenses the buffered output of a one-shot run into a single cause.
pub(crate) fn summarize_output(kind: CoreKind, output: CapturedOutput<'_>) -> String {
    let mut parser = LogParser::new(kind, 0);
    let mut frames = Vec::new();
    let mut drain = |stream, text: &str| {
        for line in text.lines() {
            frames.extend(parser.push(stream, line.to_owned()).into_iter().flatten());
        }
    };
    drain(LogStream::Stdout, output.stdout);
    drain(LogStream::Stderr, output.stderr);
    frames.extend(parser.finish().into_iter().flatten());
    error_summary(&frames).unwrap_or_else(|| verbatim_output(output))
}

fn verbatim_output(output: CapturedOutput<'_>) -> String {
    let text = [output.stdout.trim(), output.stderr.trim()]
        .into_iter()
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        "core reported no output".to_owned()
    } else {
        text
    }
}

fn stream_index(stream: LogStream) -> usize {
    match stream {
        LogStream::Stdout => 0,
        LogStream::Stderr => 1,
    }
}

/// Drops complete CSI sequences rather than bare escapes, so `[32m` cannot leak
/// into a level or target. Lines without an escape keep their allocation.
fn strip_ansi(line: String) -> String {
    if !line.as_bytes().contains(&0x1b) {
        return line;
    }
    let bytes = line.into_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0x1b {
            output.push(bytes[index]);
            index += 1;
            continue;
        }
        if bytes.get(index + 1) != Some(&b'[') {
            index += 1;
            continue;
        }
        index += 2;
        // Parameter and intermediate bytes run until the final byte, which is the
        // only ASCII byte in the sequence and therefore never splits a character.
        while index < bytes.len() && !(0x40..=0x7e).contains(&bytes[index]) {
            index += 1;
        }
        index = (index + 1).min(bytes.len());
    }
    String::from_utf8(output).expect("dropping ASCII escapes preserves UTF-8")
}

fn parse_level(level: &str) -> Option<LogLevel> {
    match level {
        "trace" | "TRC" | "TRACE" => Some(LogLevel::Trace),
        "debug" | "DBG" | "DEBUG" => Some(LogLevel::Debug),
        "info" | "INF" | "INFO" => Some(LogLevel::Info),
        "warn" | "warning" | "WRN" | "WARN" => Some(LogLevel::Warning),
        "error" | "ERR" | "ERROR" => Some(LogLevel::Error),
        "fatal" | "panic" | "FTL" | "PNC" => Some(LogLevel::Fatal),
        // phuslu/log's placeholder for a level it does not know.
        "???" => Some(LogLevel::Info),
        _ => None,
    }
}

/// logrus logfmt: `time="..." level=... msg="..."` followed by the call site's
/// own fields, all of them quoted when they need to be.
fn parse_mihomo(line: &str) -> Option<ParsedLine> {
    let mut raw_time = None;
    let mut level = None;
    let mut message = None;
    let mut fields = Vec::new();
    for (key, value) in scan_logfmt(line)? {
        match key.as_str() {
            "time" => raw_time = Some(value),
            "level" => level = parse_level(&value),
            "msg" => message = Some(value),
            _ => fields.push(LogField { key, value }),
        }
    }
    let raw_time = raw_time?;
    Some(ParsedLine {
        timestamp: Some(LogTimestamp {
            unix_ms: parse_rfc3339_ms(&raw_time),
            raw: raw_time,
            inferred: false,
        }),
        level: level?,
        target: None,
        message: message?,
        fields,
    })
}

fn scan_logfmt(line: &str) -> Option<Vec<(String, String)>> {
    let bytes = line.as_bytes();
    let mut fields = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        let key_start = index;
        while index < bytes.len() && bytes[index] != b'=' {
            if bytes[index].is_ascii_whitespace() {
                return None;
            }
            index += 1;
        }
        if index == key_start || index == bytes.len() {
            return None;
        }
        let key = line.get(key_start..index)?.to_owned();
        index += 1;

        let value = if bytes.get(index) == Some(&b'"') {
            index += 1;
            let mut value = Vec::new();
            let mut closed = false;
            while index < bytes.len() {
                match bytes[index] {
                    b'"' => {
                        index += 1;
                        closed = true;
                        break;
                    }
                    b'\\' => {
                        let escaped = *bytes.get(index + 1)?;
                        match escaped {
                            b'"' | b'\\' => value.push(escaped),
                            b'n' => value.push(b'\n'),
                            b'r' => value.push(b'\r'),
                            b't' => value.push(b'\t'),
                            _ => value.extend_from_slice(&[b'\\', escaped]),
                        }
                        index += 2;
                    }
                    byte => {
                        value.push(byte);
                        index += 1;
                    }
                }
            }
            if !closed
                || bytes
                    .get(index)
                    .is_some_and(|byte| !byte.is_ascii_whitespace())
            {
                return None;
            }
            String::from_utf8(value).ok()?
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            line.get(value_start..index)?.to_owned()
        };
        fields.push((key, value));
    }
    Some(fields)
}

/// tracing-subscriber's default layout:
/// `<RFC3339 UTC> <right-aligned level> <target>: message`.
fn parse_meow(line: &str) -> Option<ParsedLine> {
    let (raw_time, rest) = take_token(line)?;
    let (level, rest) = take_token(rest)?;
    let level = parse_level(level)?;
    let (target, message) = rest.split_once(": ")?;
    if target.is_empty() {
        return None;
    }
    Some(ParsedLine {
        timestamp: Some(LogTimestamp {
            unix_ms: parse_rfc3339_ms(raw_time),
            raw: raw_time.to_owned(),
            inferred: false,
        }),
        level,
        target: Some(target.to_owned()),
        message: message.to_owned(),
        fields: Vec::new(),
    })
}

/// `<timestamp> <LEVEL> [ThreadId(N)] [target] <file:line>: message`. The two
/// middle segments only exist in debug builds of the core, so the header ends at
/// the first `file:line` anchor rather than at a fixed offset from the level.
fn parse_clash_rs(line: &str, observed_at: DateTime<FixedOffset>) -> Option<ParsedLine> {
    let timestamp_end = clash_rs_timestamp_end(line)?;
    let raw_time = line.get(..timestamp_end)?;
    let (level, rest) = take_token(line.get(timestamp_end..)?)?;
    let level = parse_level(level)?;
    let (target, message) = clash_rs_source(rest)?;
    Some(ParsedLine {
        timestamp: Some(LogTimestamp {
            unix_ms: clash_rs_unix_ms(raw_time, observed_at.offset()),
            raw: raw_time.to_owned(),
            inferred: true,
        }),
        level,
        target: Some(target.to_owned()),
        message: message.to_owned(),
        fields: Vec::new(),
    })
}

/// `yy-MM-dd HH:mm:ss:<subsecond>`. The separator before the subsecond is a
/// colon and `time`'s `[subsecond]` prints 1 to 9 digits, so no RFC3339 parser
/// applies.
fn clash_rs_timestamp_end(line: &str) -> Option<usize> {
    const SEPARATORS: [(usize, u8); 6] = [
        (2, b'-'),
        (5, b'-'),
        (8, b' '),
        (11, b':'),
        (14, b':'),
        (17, b':'),
    ];
    const NUMBERS: [(usize, usize); 6] = [(0, 2), (3, 5), (6, 8), (9, 11), (12, 14), (15, 17)];
    let bytes = line.as_bytes();
    if !SEPARATORS
        .iter()
        .all(|(index, separator)| bytes.get(*index) == Some(separator))
        || !NUMBERS
            .iter()
            .all(|(start, end)| bytes[*start..*end].iter().all(u8::is_ascii_digit))
    {
        return None;
    }
    let digits = bytes[18..]
        .iter()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    // The trailing space is what separates the header from the level; without it
    // an arbitrary line of the same shape would be accepted as a record root.
    ((1..=9).contains(&digits) && bytes.get(18 + digits) == Some(&b' ')).then_some(18 + digits)
}

fn clash_rs_unix_ms(raw: &str, offset: &FixedOffset) -> Option<i64> {
    let number = |range: std::ops::Range<usize>| raw.get(range)?.parse::<u32>().ok();
    let subsecond = raw.get(18..)?;
    let nanos = subsecond.parse::<u32>().ok()? * 10_u32.pow(9 - subsecond.len() as u32);
    let date = NaiveDate::from_ymd_opt(2000 + number(0..2)? as i32, number(3..5)?, number(6..8)?)?;
    let time =
        NaiveTime::from_hms_nano_opt(number(9..11)?, number(12..14)?, number(15..17)?, nanos)?;
    local_unix_ms(date, time, offset)
}

/// Scans forward: neither `ThreadId(N)` nor a tracing target can look like
/// `<path>:<line>: `, so the first match closes the header, while a later one
/// may well be an ordinary `file:line:` reference inside the message.
fn clash_rs_source(rest: &str) -> Option<(&str, &str)> {
    for (separator, _) in rest.match_indices(": ") {
        let head = &rest[..separator];
        let Some(colon) = head.rfind(':') else {
            continue;
        };
        if head[colon + 1..].is_empty()
            || !head[colon + 1..].bytes().all(|byte| byte.is_ascii_digit())
        {
            continue;
        }
        let start = head[..colon]
            .rfind(char::is_whitespace)
            .map_or(0, |index| index + 1);
        if start == colon {
            continue;
        }
        return Some((&head[start..], &rest[separator + 2..]));
    }
    None
}

/// `HH:MM:SS LVL message (key=value)*`. The trailing pairs are printed without
/// quoting and their values may contain spaces and colons, so they stay in the
/// message instead of becoming fields.
fn parse_premium(
    line: &str,
    observed_at: DateTime<FixedOffset>,
    clock: &mut Option<(NaiveDate, NaiveTime)>,
) -> Option<ParsedLine> {
    let bytes = line.as_bytes();
    if bytes.get(8) != Some(&b' ') || bytes.get(12) != Some(&b' ') {
        return None;
    }
    let raw_time = line.get(..8)?;
    let time = NaiveTime::parse_from_str(raw_time, "%H:%M:%S").ok()?;
    let level = parse_level(line.get(9..12)?)?;
    let (target, message) = premium_target(line.get(13..)?);
    Some(ParsedLine {
        timestamp: Some(LogTimestamp {
            unix_ms: premium_unix_ms(time, observed_at, clock),
            raw: raw_time.to_owned(),
            inferred: true,
        }),
        level,
        target,
        message,
        fields: Vec::new(),
    })
}

/// The `[Tag]` prefix is a literal the call site writes into the message, not a
/// structured field, so failing to find one is normal.
fn premium_target(body: &str) -> (Option<String>, String) {
    let tagged = body
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "));
    match tagged {
        Some((tag, message)) if !tag.is_empty() && !tag.contains(' ') => {
            (Some(tag.to_owned()), message.to_owned())
        }
        _ => (None, body.to_owned()),
    }
}

/// premium prints no date. The first line adopts the observed one — a printed
/// time more than half a day ahead of it belongs to the previous day — and later
/// lines roll forward when the clock falls from end-of-day back to start-of-day.
fn premium_unix_ms(
    time: NaiveTime,
    observed_at: DateTime<FixedOffset>,
    clock: &mut Option<(NaiveDate, NaiveTime)>,
) -> Option<i64> {
    let date = match *clock {
        Some((date, previous)) if previous.signed_duration_since(time) > Duration::hours(12) => {
            date.succ_opt()?
        }
        Some((date, _)) => date,
        None => {
            let observed = observed_at.date_naive();
            if time.signed_duration_since(observed_at.time()) > Duration::hours(12) {
                observed.pred_opt()?
            } else {
                observed
            }
        }
    };
    *clock = Some((date, time));
    local_unix_ms(date, time, observed_at.offset())
}

fn local_unix_ms(date: NaiveDate, time: NaiveTime, offset: &FixedOffset) -> Option<i64> {
    offset
        .from_local_datetime(&date.and_time(time))
        .single()
        .map(|timestamp| timestamp.timestamp_millis())
}

fn parse_rfc3339_ms(raw: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|timestamp| timestamp.timestamp_millis())
}

fn take_token(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let end = input.find(char::is_whitespace)?;
    Some((&input[..end], input[end..].trim_start()))
}
