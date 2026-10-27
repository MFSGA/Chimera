export type FrontendErrorCapture = {
  status: 'captured' | 'not-installed' | 'session-unavailable';
  events: Array<{ type: string; message: string; at: number }>;
};

const sensitiveFieldPattern =
  /(["'\\]*\b(?:access[_-]?token|refresh[_-]?token|token|secret|password|authorization|api[_-]?key|cookie|set-cookie)\b["'\\]*\s*[:=]\s*)(?:"(?:\\.|[^"\\])*"|'(?:\\.|[^'\\])*'|[^,;}\]]+)/gi;

export function sanitizeFrontendErrorMessage(message: string): string {
  return message
    .replace(/([a-z][a-z\d+.-]*:\/\/)[^/@\s]+:[^/@\s]+@/gi, '$1[redacted]@')
    .replace(
      /([?&](?:access[_-]?token|refresh[_-]?token|token|key|secret|password|auth|api[_-]?key)=)[^&#\s]*/gi,
      '$1[redacted]',
    )
    .replace(/\bBearer\s+[^\s,;}\]]+/gi, 'Bearer [redacted]')
    .replace(sensitiveFieldPattern, '$1[redacted]')
    .replace(
      /\beyJ[a-z\d_-]{5,}\.[a-z\d_-]{5,}\.[a-z\d_-]{5,}\b/gi,
      '[redacted-jwt]',
    )
    .slice(0, 2_000);
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
