import type { CoreControllerInfo } from '@chimera/interface';

/**
 * Same channel labels as ref, sourced from Chimera's authoritative CoreActor
 * projection rather than the legacy status tuple that has no controller.
 */
export function coreControllerChannel(
  controller: CoreControllerInfo | null | undefined,
): string {
  if (!controller) return '—';
  if ('Http' in controller) return 'HTTP';
  if ('NamedPipe' in controller) return 'IPC · Named Pipe';
  if ('UnixSocket' in controller) return 'IPC · Unix Socket';
  return '—';
}
