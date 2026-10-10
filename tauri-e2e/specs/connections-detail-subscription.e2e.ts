import assert from 'node:assert/strict';

/*
 * Contract CONN-DETAIL-CHANNEL (desktop IPC integration, not UI E2E):
 * In the isolated E2E legacy webview, register a native Channel callback,
 * subscribe to the actor's on-demand detail watch, then unsubscribe it.
 * The returned subscription must be a nonnegative integer; both commands
 * must return successfully even before a proxy core has been started.
 * Removing command registration, breaking Channel argument serialization,
 * or rejecting a same-window unsubscribe fails the test. Does not assert
 * actual connection frames, UI visibility, real network traffic, or recording.
 * The callback and backend subscription are owned and cleaned by this test.
 */
describe('on-demand connection details native IPC', () => {
  it('registers and releases a native detail Channel in the owning webview', async () => {
    const windows = await browser.getWindowHandles();
    assert.ok(windows.includes('legacy'), 'E2E legacy window is required');
    await browser.switchToWindow('legacy');

    const observed = await browser.execute(async () => {
      type TauriInternals = {
        transformCallback(callback: (data: unknown) => void): number;
        unregisterCallback(id: number): void;
        invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
      };
      const tauri = (
        window as typeof window & {
          __TAURI_INTERNALS__?: TauriInternals;
        }
      ).__TAURI_INTERNALS__;
      if (!tauri) throw new Error('Tauri IPC internals are unavailable');

      // Mirrors Tauri's Channel serialization. The test uses raw IPC to prove
      // the native command wiring, not the user-facing React Hook.
      const callback = tauri.transformCallback(() => {});
      let subscription: number | undefined;
      let released = false;
      try {
        subscription = await tauri.invoke<number>(
          'subscribe_clash_connection_details',
          { onFrame: `__CHANNEL__:${callback}` },
        );
        if (!Number.isSafeInteger(subscription) || subscription < 0) {
          throw new Error(`Invalid detail subscription id: ${subscription}`);
        }
        await tauri.invoke('unsubscribe_clash_connection_details', {
          id: subscription,
        });
        released = true;
        return { subscription, released };
      } finally {
        if (subscription !== undefined && !released) {
          await tauri.invoke('unsubscribe_clash_connection_details', {
            id: subscription,
          });
        }
        tauri.unregisterCallback(callback);
      }
    });

    assert.ok(Number.isSafeInteger(observed.subscription));
    assert.ok(observed.subscription >= 0);
    assert.equal(observed.released, true);
  });
});
