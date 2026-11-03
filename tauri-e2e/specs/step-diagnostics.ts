/**
 * Minimal, privacy-safe checkpoints for the single-runner Windows UI suite.
 * Mocha's 120-second timeout does not identify the WebDriver command that is
 * awaiting completion. Emit a static stage before each major user operation,
 * so the last stage in CI logs identifies where to investigate next.
 *
 * Do not include typed input, user data, identifiers, URLs, or state values.
 */
export function markE2eStep(suite: string, stage: string): void {
  process.stdout.write(`[E2E step] ${suite}: ${stage}\n`);
}
