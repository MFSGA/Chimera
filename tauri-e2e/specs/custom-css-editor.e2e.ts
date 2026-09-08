import assert from 'node:assert/strict';

async function waitForApp() {
  await browser.waitUntil(
    async () =>
      browser.execute(
        () => (document.getElementById('root')?.childElementCount ?? 0) > 0,
      ),
    { timeout: 30_000, timeoutMsg: 'The Chimera frontend did not render.' },
  );
}

async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return browser.execute(
    async (name, payload) => {
      const tauri = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: (
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<T>;
          };
        }
      ).__TAURI_INTERNALS__;
      return tauri.invoke(name, payload);
    },
    command,
    args,
  );
}

async function waitForWindow(label: string): Promise<void> {
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes(label),
    { timeout: 15_000, timeoutMsg: `The ${label} window was not created.` },
  );
}

async function waitForWindowClosed(label: string): Promise<void> {
  await browser.waitUntil(
    async () => !(await browser.getWindowHandles()).includes(label),
    { timeout: 15_000, timeoutMsg: `The ${label} window was not closed.` },
  );
}

describe('custom CSS editor parity', () => {
  it('opens the ref-compatible singleton CSS editor route and closes cleanly', async () => {
    await waitForApp();
    const sourceWindow = (await browser.getWindowHandles()).includes('legacy')
      ? 'legacy'
      : 'main';

    await browser.switchToWindow(sourceWindow);
    await invoke<null>('create_editor_window', {
      windowType: 'css-editor',
      uid: null,
    });
    await waitForWindow('editor-css');

    const handlesAfterFirstOpen = await browser.getWindowHandles();
    await invoke<null>('create_editor_window', {
      windowType: 'css-editor',
      uid: null,
    });
    const handlesAfterSecondOpen = await browser.getWindowHandles();
    assert.deepEqual(
      [...handlesAfterSecondOpen].sort(),
      [...handlesAfterFirstOpen].sort(),
      'CSS editor must stay singleton when opened repeatedly.',
    );

    await browser.switchToWindow('editor-css');
    await waitForApp();

    const pathname = await browser.execute(() => window.location.pathname);
    assert.equal(pathname.replace(/\/$/, ''), '/editor/css');

    const editorContent = await $('[data-slot="editor-content"]');
    await editorContent.waitForDisplayed({ timeout: 30_000 });
    const footer = await $('[data-slot="editor-footer-actions"]');
    await footer.waitForDisplayed({ timeout: 15_000 });
    const monaco = await $('.monaco-editor');
    await monaco.waitForDisplayed({ timeout: 30_000 });

    const footerButtons = await footer.$$('button');
    assert.equal(footerButtons.length, 4);
    const cancel = footerButtons[1];
    await cancel.waitForClickable({ timeout: 15_000 });
    await cancel.click();
    await waitForWindowClosed('editor-css');
    await browser.switchToWindow(sourceWindow);
  });
});
