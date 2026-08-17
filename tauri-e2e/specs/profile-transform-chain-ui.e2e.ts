import assert from 'node:assert/strict';

type MutationOutcome<T> =
  | { status: 'applied'; value: T }
  | {
      status: 'committed_degraded';
      value: T;
      degradations: Array<{ message: string }>;
    };

type LogSpan = 'log' | 'info' | 'warn' | 'error';

type RuntimeTransformDiagnostics = {
  revision: number;
  output: {
    scopes: Record<string, Record<string, Array<[LogSpan, string]>>>;
    global: Record<string, Array<[LogSpan, string]>>;
  };
};

type ProfileResponse = {
  uid: string;
  name: string;
  type: 'remote' | 'local' | 'merge' | 'script';
  chain?: string[];
};

type ProfilesResponse = {
  current: string | null;
  items: ProfileResponse[];
  global_transforms: string[];
};

async function invoke<T>(command: string, args?: Record<string, unknown>) {
  return browser.execute(
    async (name, parameters) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: <R>(
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<R>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke<T>(name, parameters);
    },
    command,
    args,
  );
}

function requireApplied<T>(outcome: MutationOutcome<T>, operation: string): T {
  assert.equal(
    outcome.status,
    'applied',
    `${operation} degraded: ${
      outcome.status === 'committed_degraded'
        ? outcome.degradations.map((item) => item.message).join('; ')
        : 'unknown outcome'
    }`,
  );
  return outcome.value;
}

async function openMainWindow() {
  await invoke('create_main_window');
  await browser.waitUntil(
    async () => (await browser.getWindowHandles()).includes('main'),
    { timeout: 15_000, timeoutMsg: 'The main window was not created.' },
  );
  await browser.switchToWindow('main');
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
      const profiles = await invoke<ProfilesResponse>('get_profiles');
      const profile = profiles.items.find((item) => item.uid === uid);
      return JSON.stringify(profile?.chain ?? []) === JSON.stringify(expected);
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
      const profiles = await invoke<ProfilesResponse>('get_profiles');
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
      return !(await currentEditor.isDisplayed().catch(() => false));
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
  let previousCurrent: string | null = null;
  let localUid: string | null = null;
  let mergeAUid: string | null = null;
  let mergeBUid: string | null = null;
  let javascriptUid: string | null = null;

  before(async () => {
    await browser.setWindowSize(1240, 720);
    const initial = await invoke<ProfilesResponse>('get_profiles');
    previousCurrent = initial.current;

    requireApplied(
      await invoke<MutationOutcome<null>>('set_global_transform_chain', {
        transforms: [],
      }),
      'global chain reset',
    );

    localUid = requireApplied(
      await invoke<MutationOutcome<string>>('create_profile', {
        item: {
          type: 'local',
          uid: null,
          name: localName,
          file: null,
          desc: null,
          updated: null,
          symlinks: null,
          chain: [],
        },
        fileData: 'proxies: []\nproxy-groups: []\nrules: []\n',
      }),
      'local profile creation',
    );
    mergeAUid = requireApplied(
      await invoke<MutationOutcome<string>>('create_profile', {
        item: { type: 'merge', name: mergeAName, desc: null },
        fileData: '{}\n',
      }),
      'first merge profile creation',
    );
    mergeBUid = requireApplied(
      await invoke<MutationOutcome<string>>('create_profile', {
        item: { type: 'merge', name: mergeBName, desc: null },
        fileData: '{}\n',
      }),
      'second merge profile creation',
    );
    javascriptUid = requireApplied(
      await invoke<MutationOutcome<string>>('create_profile', {
        item: {
          type: 'script',
          name: javascriptName,
          desc: null,
          script_type: 'javascript',
        },
        fileData: [
          'export default function (config) {',
          '  console.info("chain ui javascript log");',
          '  return config;',
          '}',
          '',
        ].join('\n'),
      }),
      'JavaScript profile creation',
    );
    requireApplied(
      await invoke<MutationOutcome<null>>('activate_profile', {
        uid: localUid,
      }),
      'local profile activation',
    );

    await openMainWindow();
    await browser.setWindowSize(1240, 720);
  });

  after(async () => {
    await invoke<MutationOutcome<null>>('set_global_transform_chain', {
      transforms: [],
    }).catch(() => undefined);
    if (localUid) {
      await invoke<MutationOutcome<null>>('set_profile_transform_chain', {
        uid: localUid,
        transforms: [],
      }).catch(() => undefined);
    }
    for (const uid of [javascriptUid, mergeAUid, mergeBUid, localUid]) {
      if (!uid) continue;
      await invoke<MutationOutcome<null>>('delete_profile', { uid }).catch(
        () => undefined,
      );
    }
    if (previousCurrent) {
      await invoke<MutationOutcome<null>>('activate_profile', {
        uid: previousCurrent,
      }).catch(() => undefined);
    }
  });

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

  it('edits the global transform chain and shows applied runtime diagnostics', async () => {
    assert.ok(localUid && mergeAUid && javascriptUid);
    requireApplied(
      await invoke<MutationOutcome<null>>('set_profile_transform_chain', {
        uid: localUid,
        transforms: [],
      }),
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
    let runtimeLog = await javascriptRow.$(
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
  });
});
