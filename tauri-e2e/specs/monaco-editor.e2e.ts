import assert from 'node:assert/strict';
import { Key } from 'webdriverio';
import type { MutationOutcome } from '../../frontend/interface/src/ipc/bindings.js';
import {
  appliedValue,
  committedValue,
  createProfile,
  invoke,
  localConfigProfileRequest,
  readProfiles,
  withCleanup,
} from './profile-fixtures.js';

const editorSelector = '.monaco-editor';
const diffSelector = '.monaco-diff-editor';

type EditorDiagnostics = Window & {
  __chimeraMonacoErrors?: string[];
  __chimeraMonacoCleanup?: () => void;
};

async function monitorEditorErrors() {
  await browser.execute(() => {
    const target = window as EditorDiagnostics;
    target.__chimeraMonacoCleanup?.();
    const errors: string[] = [];
    target.__chimeraMonacoErrors = errors;
    const pattern =
      /Could not create web worker|Missing requestHandler|TextModel got disposed|reading ['"]label['"]|already exists.*model|model.*already exists/i;
    const capture = (value: unknown) => {
      const message = value instanceof Error ? value.message : String(value);
      // Report only editor-specific failures; never capture profile contents.
      if (pattern.test(message)) {
        errors.push('Monaco lifecycle or worker error');
      }
    };
    const warn = console.warn;
    const error = console.error;
    console.warn = (...args) => {
      args.forEach(capture);
      warn.apply(console, args);
    };
    console.error = (...args) => {
      args.forEach(capture);
      error.apply(console, args);
    };
    const onError = (event: ErrorEvent) =>
      capture(event.error ?? event.message);
    const onRejection = (event: PromiseRejectionEvent) => capture(event.reason);
    window.addEventListener('error', onError);
    window.addEventListener('unhandledrejection', onRejection);
    target.__chimeraMonacoCleanup = () => {
      console.warn = warn;
      console.error = error;
      window.removeEventListener('error', onError);
      window.removeEventListener('unhandledrejection', onRejection);
      delete target.__chimeraMonacoCleanup;
      delete target.__chimeraMonacoErrors;
    };
  });
}

async function assertNoEditorErrors() {
  const errors = await browser.execute(
    () => (window as EditorDiagnostics).__chimeraMonacoErrors,
  );
  assert.ok(errors, 'Editor error monitor was lost');
  assert.deepEqual(errors, []);
}

async function closeEditor(selector: string) {
  const closeTestId =
    selector === diffSelector
      ? 'runtime-config-diff-close'
      : 'profile-dialog-close';
  const close = await $(`[data-testid="${closeTestId}"]`);
  await close.waitForClickable({ timeout: 15_000 });
  await close.click();
  await $(selector).waitForExist({ reverse: true, timeout: 15_000 });
  await close.waitForExist({ reverse: true, timeout: 15_000 });
}

describe('legacy Monaco YAML worker and diff lifecycle', () => {
  it('validates edited YAML and repeatedly releases the runtime diff models', async function () {
    this.timeout(180_000);
    if (!(await browser.getWindowHandles()).includes('legacy')) {
      await invoke('create_legacy_window');
      await browser.waitUntil(
        async () => (await browser.getWindowHandles()).includes('legacy'),
        { timeout: 15_000, timeoutMsg: 'Legacy window was not created' },
      );
    }
    await browser.switchToWindow('legacy');
    const url = new URL(await browser.getUrl());
    url.pathname = '/profiles';
    url.search = '';
    await browser.url(url.href);

    const original = await readProfiles();
    const name = `monaco-contract-${Date.now()}`;
    let uid: string | undefined;

    await withCleanup('Monaco editor regression', async (defer) => {
      defer('remove browser error monitor and close any dialog', async () => {
        try {
          if (await $(diffSelector).isExisting()) {
            await closeEditor(diffSelector);
          } else if (await $(editorSelector).isExisting()) {
            await closeEditor(editorSelector);
          }
        } finally {
          await browser.execute(() =>
            (window as EditorDiagnostics).__chimeraMonacoCleanup?.(),
          );
        }
      });
      defer('restore original active profile', async () => {
        if ((await readProfiles()).current !== original.current) {
          appliedValue(
            await invoke<MutationOutcome<null>>('activate_profile', {
              uid: original.current,
            }),
            'restore original profile',
          );
        }
      });
      defer('delete only the test profile', async () => {
        const profiles = await readProfiles();
        const created = profiles.items.find(
          (item) => item.uid === uid || item.name === name,
        );
        if (!created) return;
        assert.notEqual(profiles.current, created.uid);
        committedValue(
          await invoke<MutationOutcome<null>>('delete_profile', {
            uid: created.uid,
          }),
          'delete editor fixture',
        );
      });

      uid = committedValue(
        await createProfile(
          localConfigProfileRequest(name),
          'mode: rule\nproxies: []\nproxy-groups: []\nrules:\n  - MATCH,DIRECT\n',
        ),
        'create editor fixture',
      );
      if ((await readProfiles()).current !== uid) {
        appliedValue(
          await invoke<MutationOutcome<null>>('activate_profile', { uid }),
          'activate editor fixture',
        );
      }

      await monitorEditorErrors();

      const card = await $(
        `[data-slot="profile-item"][data-profile-uid="${uid}"]`,
      );
      await card.waitForDisplayed({ timeout: 30_000 });
      const menu = await card.$('[data-slot="profile-menu-trigger"]');
      await menu.waitForClickable({ timeout: 15_000 });
      await menu.click();
      const edit = await $('[data-slot="profile-edit-menu-item"]');
      await edit.waitForClickable({ timeout: 15_000 });
      await edit.click();

      const editor = await $(editorSelector);
      await editor.waitForDisplayed({ timeout: 30_000 });
      const line = await editor.$('.view-lines');
      await browser.waitUntil(
        async () => (await line.getText()).includes('MATCH,DIRECT'),
        {
          timeout: 15_000,
          timeoutMsg: 'Fixture YAML did not render',
        },
      );
      await line.click();
      await browser.keys([Key.Ctrl, 'a']);
      await browser.keys(Key.NULL);
      // A syntactically valid document with a schema-invalid rules field.
      // A real YAML worker must return a validation marker, not merely render.
      await browser.keys('rules: 42');
      await browser.waitUntil(
        async () =>
          (await editor.$$('.squiggly-error, .squiggly-warning').length) > 0,
        {
          timeout: 20_000,
          timeoutMsg: 'YAML worker did not produce a schema validation marker',
        },
      );
      await assertNoEditorErrors();
      await closeEditor(editorSelector);

      // Do not save the invalid draft. The active fixture remains unchanged.
      assert.equal((await readProfiles()).current, uid);
      for (let i = 0; i < 20; i++) {
        const trigger = await $('button[title="Runtime Config"]');
        await trigger.waitForClickable({ timeout: 15_000 });
        await trigger.click();
        const diff = await $(diffSelector);
        await diff.waitForDisplayed({ timeout: 30_000 });
        await browser.waitUntil(
          async () => (await diff.$$(editorSelector).length) === 2,
          { timeout: 15_000, timeoutMsg: 'Diff models did not mount' },
        );
        await closeEditor(diffSelector);
        // Every reopen uses the same model URIs. Leaked models would prevent
        // recreation with those URIs or trigger a Monaco lifecycle error.
        await assertNoEditorErrors();
      }
    });
  });
});
