/** A connected controller may still be opening its separate details stream.
 * Reflect both stages instead of showing "waiting for data" too early. */
export function connectionWidgetStreamState(
  connector: 'disconnected' | 'connecting' | 'connected',
  stream: 'idle' | 'connecting' | 'connected' | 'error',
): 'disconnected' | 'connecting' | 'error' | 'ready' {
  if (connector === 'disconnected') return 'disconnected';
  if (connector === 'connecting') return 'connecting';
  if (stream === 'connecting') return 'connecting';
  if (stream === 'error') return 'error';
  return 'ready';
}
