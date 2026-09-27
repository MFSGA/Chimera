import assert from 'node:assert/strict';
import type {
  MutationOutcome,
  ProfileDocument_Deserialize,
} from '../../frontend/interface/src/ipc/bindings.js';
import {
  appliedValue,
  committedValue,
  createProfile,
  deleteProfile,
  invoke,
  localConfigProfileRequest,
  overlayProfileRequest,
  readProfiles,
  runCleanupActions,
  scopedTransformsOf,
  scriptProfileRequest,
  setGlobalTransforms,
  setScopedTransforms,
  type CleanupAction,
} from './profile-fixtures.js';

type LogSpan = 'log' | 'info' | 'warn' | 'error';

type RuntimeTransformDiagnostics = {
  revision: number;
  output: {
    scopes: Record<string, Record<string, Array<[LogSpan, string]>>>;
    global: Record<string, Array<[LogSpan, string]>>;
  };
  failure: {
    attempt_revision: number;
    transform_uid: string;
    scope_uid: string | null;
    script_type: 'javascript' | 'lua' | null;
    message: string;
  } | null;
};

type CoreState = 'Running' | { Stopped: string | null };

function requireApplied<T>(outcome: MutationOutcome<T>, operation: string): T {
  return appliedValue(outcome, operation);
}

function requireCommitted<T>(
  outcome: MutationOutcome<T>,
  operation: string,
): T {
  return committedValue(outcome, operation);
}

async function waitForCoreRunning(): Promise<void> {
  let lastState = 'unknown';
  await browser.waitUntil(
    async () => {
      const [state] =
        await invoke<[CoreState, number, string]>('get_core_status');
      lastState = JSON.stringify(state);
      return state === 'Running';
    },
    {
      timeout: 30_000,
      timeoutMsg: `A running core is required for transform-chain assertions. Last state: ${lastState}`,
    },
  );
}

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
}

async function openProfileEditorWindow(uid: string) {
  const label = `profile-editor-${uid}`;
  await invoke('create_editor_window', { windowType: 'profile', uid });
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes(label),
    {
      timeout: 15_000,
      timeoutMsg: `Profile editor window ${label} was not created.`,
    },
  );
  await browser.switchToWindow(label);
  await browser.waitUntil(
    async () =>
      browser.execute(() =>
        Boolean(document.querySelector('[data-slot="profile-editor-save"]')),
      ),
    {
      timeout: 30_000,
      timeoutMsg: `Profile editor window ${label} did not render.`,
    },
  );
  return label;
}

async function openRoute(pathname: string) {
  const currentUrl = new URL(await browser.getUrl());
  currentUrl.pathname = pathname;
  currentUrl.search = '';
  await browser.url(currentUrl.href);
  await browser.waitUntil(
    async () =>
      browser.execute((expected) => location.pathname === expected, pathname),
    {
      timeout: 15_000,
      timeoutMsg: `Route ${pathname} did not render.`,
    },
  );
}

async function waitForScopedChain(uid: string, expected: string[]) {
  await browser.waitUntil(
    async () => {
      const profiles: ProfileDocument_Deserialize = await readProfiles();
      const profile = profiles.items.find((item) => item.uid === uid);
      return (
        JSON.stringify(profile && scopedTransformsOf(profile)) ===
        JSON.stringify(expected)
      );
    },
    {
      timeout: 30_000,
      timeoutMsg: `Scoped transform chain did not become ${JSON.stringify(expected)}.`,
    },
  );
}

async function waitForGlobalChain(expected: string[]) {
  await browser.waitUntil(
    async () => {
      const profiles = await readProfiles();
      return (
        JSON.stringify(profiles.global_transforms) === JSON.stringify(expected)
      );
    },
    {
      timeout: 30_000,
      timeoutMsg: `Global transform chain did not become ${JSON.stringify(expected)}.`,
    },
  );
}

async function waitForScopedRuntimeLog(
  uid: string,
  transformUid: string,
  expected: [LogSpan, string],
) {
  await browser.waitUntil(
    async () => {
      const latest = await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
      return (
        JSON.stringify(latest?.output.scopes[uid]?.[transformUid] ?? []) ===
        JSON.stringify([expected])
      );
    },
    {
      timeout: 30_000,
      timeoutMsg: `Scoped transform ${transformUid} did not publish ${JSON.stringify(expected)}.`,
    },
  );
  const latest = await invoke<RuntimeTransformDiagnostics | null>(
    'get_runtime_transform_diagnostics',
  );
  assert.ok(latest);
  return latest;
}

async function waitForGlobalRuntimeLog(
  transformUid: string,
  expected: [LogSpan, string],
  afterRevision = 0,
) {
  await browser.waitUntil(
    async () => {
      const latest = await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
      return (
        (latest?.revision ?? 0) > afterRevision &&
        JSON.stringify(latest?.output.global[transformUid] ?? []) ===
          JSON.stringify([expected])
      );
    },
    {
      timeout: 30_000,
      timeoutMsg: `Global transform ${transformUid} did not publish ${JSON.stringify(expected)} after revision ${afterRevision}.`,
    },
  );
  const latest = await invoke<RuntimeTransformDiagnostics | null>(
    'get_runtime_transform_diagnostics',
  );
  assert.ok(latest);
  return latest;
}

async function waitForGlobalRuntimeLogCleared(
  transformUid: string,
  afterRevision: number,
) {
  await browser.waitUntil(
    async () => {
      const latest = await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
      return (
        (latest?.revision ?? 0) > afterRevision &&
        (latest?.output.global[transformUid] ?? []).length === 0
      );
    },
    {
      timeout: 30_000,
      timeoutMsg: `Global transform ${transformUid} logs were not cleared after revision ${afterRevision}.`,
    },
  );
  const latest = await invoke<RuntimeTransformDiagnostics | null>(
    'get_runtime_transform_diagnostics',
  );
  assert.ok(latest);
  return latest;
}

async function waitForEditorClosed(scope: 'profile' | 'global') {
  await browser.waitUntil(
    async () => {
      const currentEditor = await browser.$(
        `[data-slot="transform-chain-editor"][data-chain-scope="${scope}"]`,
      );
      return !(await currentEditor.isDisplayed());
    },
    {
      timeout: 30_000,
      timeoutMsg: `${scope} transform chain editor did not close.`,
    },
  );
}

async function activeOrder(scope: 'profile' | 'global') {
  return browser.execute((chainScope) => {
    const editor = document.querySelector(
      `[data-slot="transform-chain-editor"][data-chain-scope="${chainScope}"]`,
    );
    return Array.from(
      editor?.querySelectorAll('[data-slot="transform-chain-active-item"]') ??
        [],
    ).map((row) => row.getAttribute('data-profile-uid'));
  }, scope);
}

describe('main transform chain editor', () => {
  const suffix = Date.now();
  const localName = `chain-ui-source-${suffix}`;
  const mergeAName = `chain-ui-merge-a-${suffix}`;
  const mergeBName = `chain-ui-merge-b-${suffix}`;
  const javascriptName = `chain-ui-javascript-${suffix}`;
  const javascriptFileData = [
    'export default function (config) {',
    '  console.info("chain ui javascript log");',
    '  return config;',
    '}',
    '',
  ].join('\n');
  const failingJavascriptName = `chain-ui-javascript-failing-${suffix}`;
  const failingMergeName = `chain-ui-merge-failing-${suffix}`;
  let previousGlobalTransforms: string[] = [];
  let previousCurrent: string | null = null;
  let localUid: string | undefined;
  let mergeAUid: string | undefined;
  let mergeBUid: string | undefined;
  let javascriptUid: string | undefined;
  let failingJavascriptUid: string | undefined;
  let failingMergeUid: string | undefined;
  let selectionMayHaveChanged = false;
  let globalTransformsMayHaveChanged = false;
  const stateCleanups: CleanupAction[] = [];
  const resourceCleanups: CleanupAction[] = [];

  before(async () => {
    await browser.setWindowSize(1240, 720);
    const initial = await readProfiles();
    previousCurrent = initial.current ?? null;
    previousGlobalTransforms = initial.global_transforms ?? [];
    stateCleanups.push(
      {
        label: 'restore original Profile selection',
        run: async () => {
          if (!selectionMayHaveChanged) return;
          requireCommitted(
            await invoke<MutationOutcome<null>>('activate_profile', {
              uid: previousCurrent,
            }),
            'Profile selection restoration',
          );
        },
      },
      {
        label: 'restore original global transforms',
        run: async () => {
          if (!globalTransformsMayHaveChanged) return;
          requireCommitted(
            await setGlobalTransforms(previousGlobalTransforms),
            'global transform restoration',
          );
        },
      },
      {
        label: 'clear transforms from test source',
        run: async () => {
          if (!localUid) return;
          requireCommitted(
            await setScopedTransforms(localUid, []),
            'scoped transform cleanup',
          );
        },
      },
    );
    resourceCleanups.push(
      {
        label: 'delete test source',
        run: async () => {
          const uid =
            localUid ??
            (await readProfiles()).items.find((item) => item.name === localName)
              ?.uid;
          if (!uid) return;
          requireCommitted(await deleteProfile(uid), 'source Profile deletion');
        },
      },
      ...[
        {
          label: 'delete first overlay',
          name: mergeAName,
          uid: () => mergeAUid,
        },
        {
          label: 'delete second overlay',
          name: mergeBName,
          uid: () => mergeBUid,
        },
        {
          label: 'delete JavaScript transform',
          name: javascriptName,
          uid: () => javascriptUid,
        },
        {
          label: 'delete failing JavaScript transform',
          name: failingJavascriptName,
          uid: () => failingJavascriptUid,
        },
        {
          label: 'delete failing overlay',
          name: failingMergeName,
          uid: () => failingMergeUid,
        },
      ].map(({ label, name, uid }) => ({
        label,
        run: async () => {
          const profileUid =
            uid() ??
            (await readProfiles()).items.find((item) => item.name === name)
              ?.uid;
          if (!profileUid) return;
          requireCommitted(
            await deleteProfile(profileUid),
            `${label} Profile cleanup`,
          );
        },
      })),
    );

    await waitForCoreRunning();
    globalTransformsMayHaveChanged = true;
    requireCommitted(await setGlobalTransforms([]), 'global transform reset');

    const local = await createProfile(
      localConfigProfileRequest(localName),
      'proxies: []\nproxy-groups: []\nrules: []\n',
    );
    localUid = local.value;
    requireCommitted(local, 'local Profile creation');

    const mergeA = await createProfile(
      overlayProfileRequest(mergeAName),
      '{}\n',
    );
    mergeAUid = mergeA.value;
    requireCommitted(mergeA, 'first overlay Profile creation');
    const mergeB = await createProfile(
      overlayProfileRequest(mergeBName),
      '{}\n',
    );
    mergeBUid = mergeB.value;
    requireCommitted(mergeB, 'second overlay Profile creation');

    const javascript = await createProfile(
      scriptProfileRequest(javascriptName, 'javascript'),
      javascriptFileData,
    );
    javascriptUid = javascript.value;
    requireCommitted(javascript, 'JavaScript Profile creation');
    const failingJavascript = await createProfile(
      scriptProfileRequest(failingJavascriptName, 'javascript'),
      [
        'export default function () {',
        '  throw new Error("chain ui intentional failure");',
        '}',
        '',
      ].join('\n'),
    );
    failingJavascriptUid = failingJavascript.value;
    requireCommitted(failingJavascript, 'failing JavaScript Profile creation');
    const failingMerge = await createProfile(
      overlayProfileRequest(failingMergeName),
      '- invalid\n- merge\n',
    );
    failingMergeUid = failingMerge.value;
    requireCommitted(failingMerge, 'failing overlay Profile creation');

    selectionMayHaveChanged = true;
    requireApplied(
      await invoke<MutationOutcome<null>>('activate_profile', {
        uid: localUid,
      }),
      'local Profile activation',
    );

    await openMainWindow();
    await browser.setWindowSize(1240, 720);
  });

  afterEach(async () => {
    const errors: unknown[] = [];
    let handles: string[] = [];
    try {
      handles = await browser.getWindowHandles();
    } catch (error) {
      errors.push(error);
    }
    if (handles.includes('main')) {
      try {
        await browser.switchToWindow('main');
      } catch (error) {
        errors.push(error);
      }
    }
    if (javascriptUid) {
      try {
        requireApplied(
          await invoke<MutationOutcome<null>>('save_profile_file', {
            uid: javascriptUid,
            fileData: javascriptFileData,
          }),
          'restore JavaScript transform fixture',
        );
      } catch (error) {
        errors.push(error);
      }

      const editorLabel = `profile-editor-${javascriptUid}`;
      try {
        handles = await browser.getWindowHandles();
      } catch (error) {
        errors.push(error);
      }
      if (handles.includes(editorLabel)) {
        try {
          await browser.switchToWindow(editorLabel);
          await browser.closeWindow();
          await browser.waitUntil(
            async () =>
              !(await browser.getWindowHandles()).includes(editorLabel),
            {
              timeout: 15_000,
              timeoutMsg: `Profile editor ${editorLabel} remained open after cleanup.`,
            },
          );
        } catch (error) {
          errors.push(error);
        }
      }
    }

    try {
      handles = await browser.getWindowHandles();
      if (handles.includes('main')) await browser.switchToWindow('main');
    } catch (error) {
      errors.push(error);
    }
    if (errors.length > 0) {
      throw new AggregateError(errors, 'Profile transform spec cleanup failed');
    }
  });

  after(async () => {
    await runCleanupActions('Profile transform fixtures', [
      ...stateCleanups,
      ...resourceCleanups.slice(1).reverse(),
      resourceCleanups[0],
    ]);
  });

  // Contract: with the fixture Profile active, selecting and reordering three
  // transforms in the detail editor must persist their UID order and apply the
  // JS log to the runtime; the Profile document and diagnostics are separate
  // authorities. A stale `chain` field or old create IPC fails setup/assertion.
  it('edits and reorders a scoped transform chain from profile details', async () => {
    assert.ok(localUid && mergeAUid && mergeBUid && javascriptUid);
    await openRoute(`/main/profiles/profile/detail/${localUid}`);

    const trigger = await $('[data-slot="profile-transform-chain"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();

    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const first = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${mergeAUid}"]`,
    );
    const second = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${mergeBUid}"]`,
    );
    const javascript = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${javascriptUid}"]`,
    );
    await first.click();
    await second.click();
    await javascript.click();
    assert.deepEqual(await activeOrder('profile'), [
      mergeAUid,
      mergeBUid,
      javascriptUid,
    ]);

    const secondRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${mergeBUid}"]`,
    );
    await secondRow.$('[data-slot="transform-chain-move-up"]').click();
    assert.deepEqual(await activeOrder('profile'), [
      mergeBUid,
      mergeAUid,
      javascriptUid,
    ]);

    const beforeSaveDiagnostics =
      await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForScopedChain(localUid, [mergeBUid, mergeAUid, javascriptUid]);
    const appliedDiagnostics = await waitForScopedRuntimeLog(
      localUid,
      javascriptUid,
      ['info', 'chain ui javascript log'],
    );
    assert.ok(
      appliedDiagnostics.revision > (beforeSaveDiagnostics?.revision ?? 0),
      'applied runtime revision did not advance after saving the scoped chain',
    );
    await waitForEditorClosed('profile');

    const currentTrigger = await $('[data-slot="profile-transform-chain"]');
    await currentTrigger.waitForClickable({ timeout: 15_000 });
    await currentTrigger.click();
    const currentEditor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
    );
    await currentEditor.waitForDisplayed({ timeout: 15_000 });
    const diagnostics = await currentEditor.$(
      '[data-slot="transform-runtime-diagnostics"]',
    );
    await diagnostics.waitForDisplayed({ timeout: 15_000 });
    assert.ok(await diagnostics.getAttribute('data-runtime-revision'));

    const javascriptRow = await currentEditor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${javascriptUid}"]`,
    );
    const runtimeLog = await javascriptRow.$(
      '[data-slot="transform-runtime-log"][data-log-span="info"]',
    );
    await runtimeLog.waitForDisplayed({ timeout: 15_000 });
    assert.match(await runtimeLog.getText(), /chain ui javascript log/);

    for (const mergeUid of [mergeBUid, mergeAUid]) {
      const mergeRow = await currentEditor.$(
        `[data-slot="transform-chain-active-item"][data-profile-uid="${mergeUid}"]`,
      );
      assert.equal(
        await mergeRow.$('[data-slot="transform-runtime-logs"]').isExisting(),
        false,
        `merge transform ${mergeUid} should not render an empty runtime log block`,
      );
    }
  });

  // Contract: save a failing JS transform through editor IPC while the editor
  // window stays open; backend file content and scoped diagnostics must identify
  // that script, then a valid repair clears failure. A stale success result or
  // missing runtime error fails this case.
  it('keeps the profile editor open and shows transform failures after a degraded file save', async () => {
    assert.ok(localUid && javascriptUid);
    const sourceUid = localUid;
    const transformUid = javascriptUid;
    requireApplied(
      await setGlobalTransforms([]),
      'global chain reset before profile editor diagnostics',
    );
    requireApplied(
      await setScopedTransforms(sourceUid, [transformUid]),
      'scoped JavaScript chain before profile editor diagnostics',
    );
    await waitForScopedChain(sourceUid, [transformUid]);

    const originalFile = await invoke<string>('read_profile_file', {
      uid: transformUid,
    });
    const before = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(before);

    const editorLabel = await openProfileEditorWindow(transformUid);
    const degraded = await invoke<MutationOutcome<null>>('save_profile_file', {
      uid: transformUid,
      fileData: [
        'export default function () {',
        '  throw new Error("profile editor intentional failure");',
        '}',
        '',
      ].join('\n'),
    });
    assert.equal(degraded.status, 'committed_degraded');

    await browser.refresh();
    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          Boolean(document.querySelector('[data-slot="profile-editor-save"]')),
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'Profile editor did not reload after the degraded save.',
      },
    );
    assert.match(
      await invoke<string>('read_profile_file', { uid: transformUid }),
      /profile editor intentional failure/,
    );

    const failure = await $('[data-slot="profile-editor-runtime-failure"]');
    await failure.waitForDisplayed({ timeout: 30_000 });
    assert.match(await failure.getText(), /profile editor intentional failure/);
    assert.equal(
      (await browser.getWindowHandles()).includes(editorLabel),
      true,
      'degraded profile save unexpectedly closed the editor window',
    );

    const failed = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    const runtimeFailure = failed?.failure;
    if (!runtimeFailure) {
      throw new Error('runtime failure diagnostics were not available');
    }
    assert.ok(runtimeFailure.attempt_revision > before.revision);
    assert.equal(runtimeFailure.transform_uid, transformUid);
    assert.equal(runtimeFailure.scope_uid, sourceUid);

    requireApplied(
      await invoke<MutationOutcome<null>>('save_profile_file', {
        uid: transformUid,
        fileData: originalFile,
      }),
      'profile editor script repair',
    );
    await browser.refresh();
    await browser.waitUntil(
      async () => {
        const currentFailure = await $(
          '[data-slot="profile-editor-runtime-failure"]',
        );
        return !(await currentFailure.isExisting());
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Profile editor kept stale diagnostics after script repair.',
      },
    );

    await browser.closeWindow();
    await browser.waitUntil(
      async () => !(await browser.getWindowHandles()).includes(editorLabel),
      {
        timeout: 15_000,
        timeoutMsg:
          'Profile editor window remained after the degraded save check.',
      },
    );
    await browser.switchToWindow('main');
  });

  // Contract: saving overlay and JS IDs through the global editor must update
  // persisted order and visible diagnostics; an external IPC update refreshes
  // an already open editor, then clears logs.
  it('edits the global transform chain and shows applied runtime diagnostics', async () => {
    assert.ok(localUid && mergeAUid && javascriptUid);
    requireApplied(
      await setScopedTransforms(localUid, []),
      'scoped chain reset before global diagnostics',
    );
    await waitForScopedChain(localUid, []);
    await openRoute('/main/profiles/merge');

    const trigger = await $('[data-slot="global-transform-chain"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();

    let editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });
    await editor
      .$(
        `[data-slot="transform-chain-inactive-item"][data-profile-uid="${mergeAUid}"]`,
      )
      .click();
    await editor
      .$(
        `[data-slot="transform-chain-inactive-item"][data-profile-uid="${javascriptUid}"]`,
      )
      .click();
    assert.deepEqual(await activeOrder('global'), [mergeAUid, javascriptUid]);

    const beforeSaveDiagnostics =
      await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid, javascriptUid]);
    const firstApplied = await waitForGlobalRuntimeLog(
      javascriptUid,
      ['info', 'chain ui javascript log'],
      beforeSaveDiagnostics?.revision ?? 0,
    );
    await waitForEditorClosed('global');

    let currentTrigger = await $('[data-slot="global-transform-chain"]');
    await currentTrigger.waitForClickable({ timeout: 15_000 });
    await currentTrigger.click();
    editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    let javascriptRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${javascriptUid}"]`,
    );
    const runtimeLog = await javascriptRow.$(
      '[data-slot="transform-runtime-log"][data-log-span="info"]',
    );
    await runtimeLog.waitForDisplayed({ timeout: 15_000 });
    assert.match(await runtimeLog.getText(), /chain ui javascript log/);
    const diagnostics = await editor.$(
      '[data-slot="transform-runtime-diagnostics"]',
    );
    assert.equal(
      Number(await diagnostics.getAttribute('data-runtime-revision')),
      firstApplied.revision,
    );

    await javascriptRow.$('[data-slot="transform-chain-move-up"]').click();
    assert.deepEqual(await activeOrder('global'), [javascriptUid, mergeAUid]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([javascriptUid, mergeAUid]);
    const reordered = await waitForGlobalRuntimeLog(
      javascriptUid,
      ['info', 'chain ui javascript log'],
      firstApplied.revision,
    );
    await waitForEditorClosed('global');

    currentTrigger = await $('[data-slot="global-transform-chain"]');
    await currentTrigger.waitForClickable({ timeout: 15_000 });
    await currentTrigger.click();
    editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });
    javascriptRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${javascriptUid}"]`,
    );
    await javascriptRow.$('[data-slot="transform-chain-remove"]').click();
    assert.deepEqual(await activeOrder('global'), [mergeAUid]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid]);
    const cleared = await waitForGlobalRuntimeLogCleared(
      javascriptUid,
      reordered.revision,
    );
    assert.ok(cleared.revision > reordered.revision);
    await waitForEditorClosed('global');

    currentTrigger = await $('[data-slot="global-transform-chain"]');
    await currentTrigger.waitForClickable({ timeout: 15_000 });
    await currentTrigger.click();
    editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });
    const clearedDiagnostics = await editor.$(
      '[data-slot="transform-runtime-diagnostics"]',
    );
    assert.equal(
      Number(await clearedDiagnostics.getAttribute('data-runtime-revision')),
      cleared.revision,
    );
    await editor
      .$('[data-slot="transform-runtime-diagnostics-empty"]')
      .waitForDisplayed({ timeout: 15_000 });
    const mergeRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${mergeAUid}"]`,
    );
    assert.equal(
      await mergeRow.$('[data-slot="transform-runtime-logs"]').isExisting(),
      false,
      'merge-only global chain should not render an empty runtime log block',
    );

    requireApplied(
      await setGlobalTransforms([mergeAUid, javascriptUid]),
      'external global script attachment',
    );
    await browser.waitUntil(
      async () =>
        browser.execute((afterRevision) => {
          const diagnostics = document.querySelector(
            '[data-slot="transform-chain-editor"][data-chain-scope="global"] [data-slot="transform-runtime-diagnostics"]',
          );
          const empty = document.querySelector(
            '[data-slot="transform-chain-editor"][data-chain-scope="global"] [data-slot="transform-runtime-diagnostics-empty"]',
          );
          const revision = Number(
            diagnostics?.getAttribute('data-runtime-revision') ?? 0,
          );
          return revision > afterRevision && empty === null;
        }, cleared.revision),
      {
        timeout: 30_000,
        timeoutMsg:
          'Open global transform diagnostics did not refresh after an external runtime promotion.',
      },
    );
    const externallyApplied = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(externallyApplied);
    assert.ok(externallyApplied.revision > cleared.revision);

    requireApplied(
      await setGlobalTransforms([mergeAUid]),
      'external global script detachment',
    );
    await browser.waitUntil(
      async () =>
        browser.execute((afterRevision) => {
          const diagnostics = document.querySelector(
            '[data-slot="transform-chain-editor"][data-chain-scope="global"] [data-slot="transform-runtime-diagnostics"]',
          );
          const empty = document.querySelector(
            '[data-slot="transform-chain-editor"][data-chain-scope="global"] [data-slot="transform-runtime-diagnostics-empty"]',
          );
          const revision = Number(
            diagnostics?.getAttribute('data-runtime-revision') ?? 0,
          );
          return revision > afterRevision && empty !== null;
        }, externallyApplied.revision),
      {
        timeout: 30_000,
        timeoutMsg:
          'Open global transform diagnostics did not return to the empty state after external detachment.',
      },
    );
    await editor.$('[data-slot="transform-chain-cancel"]').click();
    await waitForEditorClosed('global');
  });

  // Contract: a failing JS transform in the global chain must be attributed to
  // its UID in backend diagnostics and the editor row; removing it clears the
  // failure. Actual script execution, rather than display-only state, is the
  // failure boundary.
  it('pins a failed transform attempt to the responsible global script', async () => {
    assert.ok(mergeAUid && failingJavascriptUid);
    await openRoute('/main/profiles/merge');

    const before = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(before);
    assert.equal(before.failure, null);

    const trigger = await $('[data-slot="global-transform-chain"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();
    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingTransform = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${failingJavascriptUid}"]`,
    );
    await failingTransform.click();
    assert.deepEqual(await activeOrder('global'), [
      mergeAUid,
      failingJavascriptUid,
    ]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid, failingJavascriptUid]);
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${failingJavascriptUid}"]`,
    );
    const failure = await failingRow.$(
      '[data-slot="transform-runtime-failure"]',
    );
    await failure.waitForDisplayed({ timeout: 30_000 });
    const attemptRevision = Number(
      await failure.getAttribute('data-attempt-revision'),
    );
    assert.ok(attemptRevision > before.revision);
    assert.equal(await failure.getAttribute('data-script-type'), 'javascript');
    assert.match(await failure.getText(), /chain ui intentional failure/);

    const failedDiagnostics = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(failedDiagnostics?.failure);
    assert.equal(
      failedDiagnostics.revision,
      before.revision,
      'failed transform attempt must not replace the applied runtime revision',
    );
    assert.equal(failedDiagnostics.failure.attempt_revision, attemptRevision);
    assert.equal(failedDiagnostics.failure.transform_uid, failingJavascriptUid);
    assert.equal(failedDiagnostics.failure.scope_uid, null);
    assert.equal(failedDiagnostics.failure.script_type, 'javascript');
    assert.match(
      failedDiagnostics.failure.message,
      /chain ui intentional failure/,
    );

    const currentFailingRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${failingJavascriptUid}"]`,
    );
    await currentFailingRow.$('[data-slot="transform-chain-remove"]').click();
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid]);
    await waitForEditorClosed('global');

    await browser.waitUntil(
      async () => {
        const latest = await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
        return (
          (latest?.revision ?? 0) > before.revision && latest?.failure === null
        );
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Transform failure diagnostics did not clear after repairing the global chain.',
      },
    );
    const repaired = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(repaired);
    assert.ok(repaired.revision > before.revision);
    assert.equal(repaired.failure, null);
  });

  // Contract: a failing JS transform in the selected Profile's scoped chain is
  // attributed to the script and source UID, then clears after removal. Both
  // backend diagnostics and the editor row must reflect the result.
  it('pins a failed transform attempt to the responsible scoped script', async () => {
    assert.ok(localUid && mergeAUid && failingJavascriptUid);
    requireApplied(
      await setGlobalTransforms([]),
      'global chain reset before scoped failure diagnostics',
    );
    requireApplied(
      await setScopedTransforms(localUid, [mergeAUid]),
      'scoped chain baseline before scoped failure diagnostics',
    );
    await waitForScopedChain(localUid, [mergeAUid]);
    await openRoute(`/main/profiles/profile/detail/${localUid}`);

    const before = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(before);
    assert.equal(before.failure, null);

    const trigger = await $('[data-slot="profile-transform-chain"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();
    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingTransform = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${failingJavascriptUid}"]`,
    );
    await failingTransform.click();
    assert.deepEqual(await activeOrder('profile'), [
      mergeAUid,
      failingJavascriptUid,
    ]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForScopedChain(localUid, [mergeAUid, failingJavascriptUid]);
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${failingJavascriptUid}"]`,
    );
    const failure = await failingRow.$(
      '[data-slot="transform-runtime-failure"]',
    );
    await failure.waitForDisplayed({ timeout: 30_000 });
    const attemptRevision = Number(
      await failure.getAttribute('data-attempt-revision'),
    );
    assert.ok(attemptRevision > before.revision);
    assert.equal(await failure.getAttribute('data-script-type'), 'javascript');
    assert.match(await failure.getText(), /chain ui intentional failure/);

    const failedDiagnostics = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(failedDiagnostics?.failure);
    assert.equal(
      failedDiagnostics.revision,
      before.revision,
      'failed scoped transform attempt must not replace the applied runtime revision',
    );
    assert.equal(failedDiagnostics.failure.attempt_revision, attemptRevision);
    assert.equal(failedDiagnostics.failure.transform_uid, failingJavascriptUid);
    assert.equal(failedDiagnostics.failure.scope_uid, localUid);
    assert.equal(failedDiagnostics.failure.script_type, 'javascript');
    assert.match(
      failedDiagnostics.failure.message,
      /chain ui intentional failure/,
    );

    await failingRow.$('[data-slot="transform-chain-remove"]').click();
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForScopedChain(localUid, [mergeAUid]);
    await waitForEditorClosed('profile');

    await browser.waitUntil(
      async () => {
        const latest = await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
        return (
          (latest?.revision ?? 0) > before.revision && latest?.failure === null
        );
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Transform failure diagnostics did not clear after repairing the scoped chain.',
      },
    );
    const repaired = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(repaired);
    assert.ok(repaired.revision > before.revision);
    assert.equal(repaired.failure, null);
  });

  // Contract: editing an active script to fail refreshes the open scoped
  // diagnostics without closing the editor; restoring valid content advances
  // runtime revision and removes the failure row.
  it('refreshes diagnostics when an active script file is edited', async () => {
    assert.ok(localUid && javascriptUid);
    const sourceUid = localUid;
    const transformUid = javascriptUid;
    requireApplied(
      await setGlobalTransforms([]),
      'global chain reset before script file diagnostics',
    );
    requireApplied(
      await setScopedTransforms(sourceUid, [transformUid]),
      'scoped JavaScript baseline before script file diagnostics',
    );
    await waitForScopedChain(sourceUid, [transformUid]);
    await openRoute(`/main/profiles/profile/detail/${sourceUid}`);

    const trigger = await $('[data-slot="profile-transform-chain"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();
    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const before = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(before);
    assert.equal(before.failure, null);

    const degraded = await invoke<MutationOutcome<null>>('save_profile_file', {
      uid: transformUid,
      fileData: [
        'export default function () {',
        '  throw new Error("chain ui edited script failure");',
        '}',
        '',
      ].join('\n'),
    });
    assert.equal(degraded.status, 'committed_degraded');

    const activeRowSelector = `[data-slot="transform-chain-active-item"][data-profile-uid="${transformUid}"]`;
    await browser.waitUntil(
      async () => {
        const currentEditor = await $(
          '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
        );
        if (!(await currentEditor.isDisplayed())) {
          return false;
        }
        const row = await currentEditor.$(activeRowSelector);
        const failure = await row.$('[data-slot="transform-runtime-failure"]');
        if (!(await failure.isDisplayed())) {
          return false;
        }
        return /chain ui edited script failure/.test(await failure.getText());
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Open scoped transform diagnostics did not refresh after a failing script file save.',
      },
    );

    const failed = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(failed?.failure);
    assert.equal(
      failed.revision,
      before.revision,
      'failed script file edit must not replace the applied runtime revision',
    );
    assert.ok(failed.failure.attempt_revision > before.revision);
    assert.equal(failed.failure.transform_uid, transformUid);
    assert.equal(failed.failure.scope_uid, sourceUid);
    assert.equal(failed.failure.script_type, 'javascript');
    assert.match(failed.failure.message, /chain ui edited script failure/);

    requireApplied(
      await invoke<MutationOutcome<null>>('save_profile_file', {
        uid: transformUid,
        fileData: [
          'export default function (config) {',
          '  console.info("chain ui edited script repaired");',
          '  return config;',
          '}',
          '',
        ].join('\n'),
      }),
      'repair active JavaScript profile content',
    );

    await browser.waitUntil(
      async () => {
        const latest = await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
        if (
          !latest ||
          latest.revision <= before.revision ||
          latest.failure !== null
        ) {
          return false;
        }
        return (
          JSON.stringify(
            latest.output.scopes[sourceUid]?.[transformUid] ?? [],
          ) === JSON.stringify([['info', 'chain ui edited script repaired']])
        );
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Transform diagnostics did not recover after repairing the active script file.',
      },
    );

    await browser.waitUntil(
      async () => {
        const currentEditor = await $(
          '[data-slot="transform-chain-editor"][data-chain-scope="profile"]',
        );
        const row = await currentEditor.$(activeRowSelector);
        const failure = await row.$('[data-slot="transform-runtime-failure"]');
        const logs = await row.$('[data-slot="transform-runtime-logs"]');
        return (
          !(await failure.isExisting()) &&
          (await logs.isDisplayed()) &&
          /chain ui edited script repaired/.test(await logs.getText())
        );
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Open scoped transform diagnostics did not clear the failure after script repair.',
      },
    );

    await editor.$('[data-slot="transform-chain-cancel"]').click();
    await waitForEditorClosed('profile');
  });

  // Contract: selecting invalid overlay YAML globally must fail runtime apply
  // and attribute the failure to that overlay; removing it clears diagnostics.
  // Profile state and the runtime failure UID are independent evidence.
  it('pins an invalid merge transform to the responsible global row', async () => {
    assert.ok(failingMergeUid);
    requireApplied(
      await setGlobalTransforms([]),
      'global chain baseline before merge failure diagnostics',
    );
    await waitForGlobalChain([]);
    await openRoute('/main/profiles/merge');

    const before = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(before);
    assert.equal(before.failure, null);

    const trigger = await $('[data-slot="global-transform-chain"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();
    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingTransform = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${failingMergeUid}"]`,
    );
    await failingTransform.click();
    assert.deepEqual(await activeOrder('global'), [failingMergeUid]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([failingMergeUid]);
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${failingMergeUid}"]`,
    );
    const failure = await failingRow.$(
      '[data-slot="transform-runtime-failure"]',
    );
    await failure.waitForDisplayed({ timeout: 30_000 });
    const attemptRevision = Number(
      await failure.getAttribute('data-attempt-revision'),
    );
    assert.ok(attemptRevision > before.revision);
    assert.equal(await failure.getAttribute('data-transform-type'), 'merge');
    assert.equal(await failure.getAttribute('data-script-type'), null);
    assert.match(await failure.getText(), /YAML mapping/);

    const failedDiagnostics = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(failedDiagnostics?.failure);
    assert.equal(
      failedDiagnostics.revision,
      before.revision,
      'invalid merge attempt must not replace the applied runtime revision',
    );
    assert.equal(failedDiagnostics.failure.attempt_revision, attemptRevision);
    assert.equal(failedDiagnostics.failure.transform_uid, failingMergeUid);
    assert.equal(failedDiagnostics.failure.scope_uid, null);
    assert.equal(failedDiagnostics.failure.script_type, null);
    assert.match(failedDiagnostics.failure.message, /YAML mapping/);

    await failingRow.$('[data-slot="transform-chain-remove"]').click();
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([]);
    await waitForEditorClosed('global');

    await browser.waitUntil(
      async () => {
        const latest = await invoke<RuntimeTransformDiagnostics | null>(
          'get_runtime_transform_diagnostics',
        );
        return (
          (latest?.revision ?? 0) > before.revision && latest?.failure === null
        );
      },
      {
        timeout: 30_000,
        timeoutMsg:
          'Transform failure diagnostics did not clear after removing the invalid merge.',
      },
    );
  });
});
