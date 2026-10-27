export type FrontendErrorCapture = {
  status: 'captured' | 'not-installed' | 'session-unavailable';
  events: Array<{ type: string; message: string; at: number }>;
};

export const FRONTEND_ERROR_REDACTION_RULES = [
  {
    pattern: String.raw`([a-z][a-z\d+.-]*:\/\/)[^/@\s]+:[^/@\s]+@`,
    flags: 'gi',
    replacement: '$1[redacted]@',
  },
  {
    pattern: String.raw`([?&](?:[\w.-]*(?:token|secret|password|passwd|auth|key|cookie|credential|signature|sig|jwt|session))=)[^&#\s"',;}\]]*`,
    flags: 'gi',
    replacement: '$1[redacted]',
  },
  {
    pattern: String.raw`\bBearer\s+[^\s,;}\]]+`,
    flags: 'gi',
    replacement: 'Bearer [redacted]',
  },
  {
    pattern: String.raw`(["'\\]*[\w.-]*(?:access[_-]?token|refresh[_-]?token|id[_-]?token|session[_-]?token|client[_-]?secret|token|secret|password|passwd|authorization|auth|api[_-]?key|key|cookie|set-cookie|credential|private[_-]?key|signature|sig|jwt)\b["'\\]*\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^,;}\]]+)`,
    flags: 'gi',
    replacement: '$1[redacted]',
  },
  {
    pattern: String.raw`\beyJ[a-z\d_-]{5,}(?:\.[a-z\d_-]{8,}){2}(?:\.[a-z\d_-]{8,}\.[a-z\d_-]{8,})?\b`,
    flags: 'gi',
    replacement: '[redacted-jwt]',
  },
] as const;

export function sanitizeFrontendErrorMessage(message: string): string {
  return FRONTEND_ERROR_REDACTION_RULES.reduce(
    (sanitized, { pattern, flags, replacement }) =>
      sanitized.replace(new RegExp(pattern, flags), replacement),
    message,
  ).slice(0, 2_000);
}

export function sanitizeFrontendErrorCapture(
  capture: FrontendErrorCapture,
): FrontendErrorCapture {
  return {
    status: capture.status,
    events: capture.events.slice(0, 25).map((event) => ({
      type: event.type.slice(0, 64),
      message: sanitizeFrontendErrorMessage(event.message),
      at: Number.isFinite(event.at) ? event.at : 0,
    })),
  };
}
