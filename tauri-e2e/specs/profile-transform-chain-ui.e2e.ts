// Detailed rejection, state-preservation, and coverage contract:
// ../../docs/testing/contracts/profile-transform-candidate-rejection.md
import assert from 'node:assert/strict';
import { Key } from 'webdriverio';
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

type ScrollViewportObservation = {
  editorTop: number;
  editorHeight: number;
  visibleTop: number;
  visibleBottom: number;
  visibleHeight: number;
  scrollTop: number;
  maxScrollTop: number;
};

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

async function readScrollViewportObservation(
  selector: string,
): Promise<ScrollViewportObservation | null> {
  return browser.execute((editorSelector) => {
    const element = document.querySelector<HTMLElement>(editorSelector);
    const viewport = element?.closest<HTMLElement>(
      '[data-slot="scroll-area-viewport"]',
    );
    if (!element || !viewport) return null;
    const rect = element.getBoundingClientRect();
    const viewportRect = viewport.getBoundingClientRect();
    const visibleTop = viewportRect.top + viewport.clientTop;
    const visibleBottom = visibleTop + viewport.clientHeight;
    return {
      editorTop: Math.round(rect.top),
      editorHeight: Math.round(rect.height),
      visibleTop: Math.round(visibleTop),
      visibleBottom: Math.round(visibleBottom),
      visibleHeight: viewport.clientHeight,
      scrollTop: viewport.scrollTop,
      maxScrollTop: Math.max(0, viewport.scrollHeight - viewport.clientHeight),
    };
  }, selector);
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
  let lastChain = 'unavailable';
  await browser.waitUntil(
    async () => {
      const profiles = await readProfiles();
      lastChain = JSON.stringify(profiles.global_transforms ?? []);
      return lastChain === JSON.stringify(expected);
    },
    {
      timeout: 30_000,
      timeoutMsg: `Global transform chain did not become ${JSON.stringify(expected)}. Last value: ${lastChain}.`,
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

async function waitForApplySettled(scope: 'profile' | 'global') {
  await browser.waitUntil(
    async () => {
      const currentEditor = await browser.$(
        `[data-slot="transform-chain-editor"][data-chain-scope="${scope}"]`,
      );
      if (!(await currentEditor.isDisplayed())) return false;
      return !(await currentEditor
        .$('[data-slot="transform-chain-save"]')
        .isExisting());
    },
    {
      timeout: 30_000,
      timeoutMsg: `${scope} transform chain apply did not settle.`,
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

async function waitForActiveOrder(
  scope: 'profile' | 'global',
  expected: string[],
) {
  await browser.waitUntil(
    async () =>
      JSON.stringify(await activeOrder(scope)) === JSON.stringify(expected),
    {
      timeout: 15_000,
      timeoutMsg: `${scope} transform draft did not become ${JSON.stringify(expected)}.`,
    },
  );
}

async function moveActiveItemWithKeyboard(uid: string) {
  const item = await $(
    `[data-slot="transform-chain-active-item"][data-profile-uid="${uid}"] [data-slot="profile-chain-editor-item"]`,
  );
  await item.waitForDisplayed({ timeout: 15_000 });
  await browser.waitUntil(
    async () => (await item.getAttribute('tabindex')) === '0',
    {
      timeout: 15_000,
      timeoutMsg: `sortable Profile ${uid} did not become keyboard-focusable`,
    },
  );
  await item.click();
  assert.equal(
    await item.isFocused(),
    true,
    `sortable Profile ${uid} did not receive keyboard focus`,
  );
  await browser.keys(Key.Space);
  for (let index = 0; index < 12; index += 1) {
    await browser.keys(Key.ArrowUp);
  }
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
    await waitForCoreRunning();

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
    if (localUid) {
      try {
        requireCommitted(
          await setGlobalTransforms([]),
          'global chain test-state cleanup',
        );
        await waitForGlobalChain([]);
      } catch (error) {
        errors.push(error);
      }
      try {
        requireCommitted(
          await setScopedTransforms(localUid, []),
          'scoped chain test-state cleanup',
        );
        await waitForScopedChain(localUid, []);
      } catch (error) {
        errors.push(error);
      }
    }
    if (javascriptUid) {
      try {
        requireCommitted(
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
    } catch (error) {
      errors.push(error);
    }
    if (handles.includes('main')) {
      try {
        await browser.switchToWindow('main');
      } catch (error) {
        errors.push(error);
      }
      try {
        await browser.setWindowSize(1240, 720);
      } catch (error) {
        errors.push(error);
      }
      try {
        await browser.execute((uid) => {
          const detail = Array.from(
            document.querySelectorAll<HTMLElement>(
              '[data-slot="profile-detail"]',
            ),
          ).find((candidate) => candidate.dataset.profileUid === uid);
          const viewport = detail?.closest<HTMLElement>(
            '[data-slot="scroll-area-viewport"]',
          );
          if (viewport) viewport.scrollTop = 0;
        }, localUid ?? '');
      } catch (error) {
        errors.push(error);
      }
    }
    if (errors.length > 0) {
      throw new AggregateError(
        errors,
        `Profile transform spec cleanup failed: ${errors
          .map((error) =>
            error instanceof Error ? error.message : String(error),
          )
          .join('; ')}`,
      );
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
    await browser.setWindowSize(1240, 360);
    await openRoute(`/main/profiles/profile/detail/${localUid}`);

    const detailSelector = `[data-slot="profile-detail"][data-profile-uid="${localUid}"]`;
    const trigger = await $(
      `[data-slot="profile-transform-chain"][data-profile-uid="${localUid}"]`,
    );
    const editorSelector = `[data-slot="transform-chain-editor"][data-chain-scope="profile"][data-profile-uid="${localUid}"]`;
    const editor = await $(editorSelector);
    await $(detailSelector).waitForExist({ timeout: 15_000 });
    await editor.waitForExist({ timeout: 15_000 });

    const initialScrollTop = await browser.execute((selector) => {
      const detail = document.querySelector<HTMLElement>(selector);
      const viewport = detail?.closest<HTMLElement>(
        '[data-slot="scroll-area-viewport"]',
      );
      if (!viewport) return null;
      viewport.scrollTop = 0;
      return viewport.scrollTop;
    }, detailSelector);
    assert.equal(
      initialScrollTop,
      0,
      'The Profile detail scroll viewport must start at the top.',
    );
    await trigger.waitForClickable({ timeout: 15_000 });

    const beforeScroll = await browser.execute((selector) => {
      const element = document.querySelector<HTMLElement>(selector);
      const viewport = element?.closest<HTMLElement>(
        '[data-slot="scroll-area-viewport"]',
      );
      if (!element || !viewport) return null;
      const rect = element.getBoundingClientRect();
      const viewportRect = viewport.getBoundingClientRect();
      const visibleTop = viewportRect.top + viewport.clientTop;
      const visibleBottom = visibleTop + viewport.clientHeight;
      return {
        top: Math.round(rect.top),
        height: Math.round(rect.height),
        visibleTop: Math.round(visibleTop),
        visibleBottom: Math.round(visibleBottom),
      };
    }, editorSelector);
    assert.ok(beforeScroll, 'The scoped transform editor is missing.');
    assert.ok(
      beforeScroll.top + beforeScroll.height / 2 > beforeScroll.visibleBottom,
      `The editor center must start below the viewport: ${JSON.stringify(beforeScroll)}`,
    );

    await trigger.click();
    const immediateAfterScroll =
      await readScrollViewportObservation(editorSelector);
    assert.ok(immediateAfterScroll, 'The scoped transform editor is missing.');
    assert.ok(
      Math.abs(
        immediateAfterScroll.editorTop - immediateAfterScroll.visibleTop,
      ) <= 2,
      `The oversized editor must reach the viewport immediately after the action. Geometry: ${JSON.stringify(immediateAfterScroll)}`,
    );
    let lastAfterScroll: ScrollViewportObservation | null = null;
    try {
      await browser.waitUntil(
        async () => {
          lastAfterScroll = await readScrollViewportObservation(editorSelector);
          if (!lastAfterScroll) return false;
          if (lastAfterScroll.editorHeight > lastAfterScroll.visibleHeight) {
            return (
              Math.abs(
                lastAfterScroll.editorTop - lastAfterScroll.visibleTop,
              ) <= 2
            );
          }
          const center =
            lastAfterScroll.editorTop + lastAfterScroll.editorHeight / 2;
          return (
            center >= lastAfterScroll.visibleTop &&
            center <= lastAfterScroll.visibleBottom
          );
        },
        {
          timeout: 15_000,
          timeoutMsg:
            'Clicking the profile transform-chain action did not scroll the editor into view.',
        },
      );
    } catch (error) {
      throw new Error(
        `Clicking the profile transform-chain action did not scroll the editor into view. Last viewport observation: ${JSON.stringify(lastAfterScroll)}`,
        { cause: error },
      );
    }

    let fitBefore: ScrollViewportObservation | null = null;
    let lastFitCandidate: ScrollViewportObservation | null = null;
    for (const height of [600, 640, 680, 720, 760, 800, 840]) {
      await browser.setWindowSize(1240, height);
      const resetScrollTop = await browser.execute((selector) => {
        const detail = document.querySelector<HTMLElement>(selector);
        const viewport = detail?.closest<HTMLElement>(
          '[data-slot="scroll-area-viewport"]',
        );
        if (!viewport) return null;
        viewport.scrollTop = 0;
        return viewport.scrollTop;
      }, detailSelector);
      assert.equal(
        resetScrollTop,
        0,
        `The Profile detail scroll viewport must reset before the ${height}px fit check.`,
      );

      const candidate = await readScrollViewportObservation(editorSelector);
      lastFitCandidate = candidate;
      if (!candidate) continue;
      const editorCenter = candidate.editorTop + candidate.editorHeight / 2;
      const viewportCenter = candidate.visibleTop + candidate.visibleHeight / 2;
      const scrollNeeded = editorCenter - viewportCenter;
      if (
        candidate.editorHeight <= candidate.visibleHeight &&
        scrollNeeded > 8 &&
        scrollNeeded <= candidate.maxScrollTop
      ) {
        fitBefore = candidate;
        break;
      }
    }
    assert.ok(
      fitBefore,
      `Could not find a fitting viewport with enough scroll range to center the editor. Last geometry: ${JSON.stringify(lastFitCandidate)}`,
    );
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();

    const immediateFitAfter =
      await readScrollViewportObservation(editorSelector);
    assert.ok(immediateFitAfter, 'The scoped transform editor is missing.');
    const immediateFitViewportCenter =
      immediateFitAfter.visibleTop + immediateFitAfter.visibleHeight / 2;
    const immediateFitEditorCenter =
      immediateFitAfter.editorTop + immediateFitAfter.editorHeight / 2;
    assert.ok(
      Math.abs(immediateFitEditorCenter - immediateFitViewportCenter) <= 2,
      `The fitting editor must reach the viewport center immediately after the action. Geometry: ${JSON.stringify(immediateFitAfter)}`,
    );

    let lastFitAfter: ScrollViewportObservation | null = null;
    try {
      await browser.waitUntil(
        async () => {
          lastFitAfter = await readScrollViewportObservation(editorSelector);
          if (!lastFitAfter || !fitBefore) return false;
          const beforeCenter =
            fitBefore.visibleTop + fitBefore.visibleHeight / 2;
          const afterCenter =
            lastFitAfter.visibleTop + lastFitAfter.visibleHeight / 2;
          const editorCenter =
            lastFitAfter.editorTop + lastFitAfter.editorHeight / 2;
          return (
            Math.abs(editorCenter - afterCenter) <= 2 &&
            editorCenter >= lastFitAfter.visibleTop &&
            editorCenter <= lastFitAfter.visibleBottom &&
            lastFitAfter.scrollTop > fitBefore.scrollTop
          );
        },
        {
          timeout: 15_000,
          timeoutMsg:
            'Clicking the Profile transform-chain action did not center a fitting editor.',
        },
      );
    } catch (error) {
      throw new Error(
        `Clicking the Profile transform-chain action did not center a fitting editor. Before: ${JSON.stringify(fitBefore)}; last observation: ${JSON.stringify(lastFitAfter)}`,
        { cause: error },
      );
    }
    await browser.setWindowSize(1240, 720);

    const first = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${mergeAUid}"]`,
    );
    const second = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${mergeBUid}"]`,
    );
    const javascript = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${javascriptUid}"]`,
    );
    await first.$('[data-slot="transform-chain-add"]').click();
    await second.$('[data-slot="transform-chain-add"]').click();
    await javascript.$('[data-slot="transform-chain-add"]').click();
    const originalOrder = [mergeAUid, mergeBUid, javascriptUid];
    assert.deepEqual(await activeOrder('profile'), originalOrder);

    const firstActiveItem = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${mergeAUid}"]`,
    );
    const lastActiveItem = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${javascriptUid}"]`,
    );
    assert.equal(
      await firstActiveItem
        .$('[data-slot="transform-chain-move-up"]')
        .isEnabled(),
      false,
      'the first active transform cannot move above the start of the chain',
    );
    assert.equal(
      await lastActiveItem
        .$('[data-slot="transform-chain-move-down"]')
        .isEnabled(),
      false,
      'the last active transform cannot move below the end of the chain',
    );

    await editor.$('[data-slot="transform-chain-reset"]').click();
    await waitForActiveOrder('profile', []);
    await waitForScopedChain(localUid, []);

    for (const uid of originalOrder) {
      const item = await editor.$(
        `[data-slot="transform-chain-inactive-item"][data-profile-uid="${uid}"]`,
      );
      await item.$('[data-slot="transform-chain-add"]').click();
    }
    assert.deepEqual(await activeOrder('profile'), originalOrder);
    const keyboardOrder = [mergeBUid, mergeAUid, javascriptUid];
    await moveActiveItemWithKeyboard(mergeBUid);
    await waitForActiveOrder('profile', keyboardOrder);
    await browser.keys(Key.Escape);
    await waitForActiveOrder('profile', originalOrder);
    await waitForScopedChain(localUid, []);

    await moveActiveItemWithKeyboard(mergeBUid);
    await waitForActiveOrder('profile', keyboardOrder);
    await browser.keys(Key.Space);
    await waitForActiveOrder('profile', keyboardOrder);

    const beforeSaveDiagnostics =
      await invoke<RuntimeTransformDiagnostics | null>(
        'get_runtime_transform_diagnostics',
      );
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForScopedChain(localUid, keyboardOrder);
    const appliedDiagnostics = await waitForScopedRuntimeLog(
      localUid,
      javascriptUid,
      ['info', 'chain ui javascript log'],
    );
    assert.ok(
      appliedDiagnostics.revision > (beforeSaveDiagnostics?.revision ?? 0),
      'applied runtime revision did not advance after saving the scoped chain',
    );
    await waitForApplySettled('profile');

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

  // Contract: a failing JS transform edit is rejected before file commit; the
  // editor stays open, diagnostics identify the failed attempt, and a valid
  // repair applies without replacing the prior runtime on failure.
  it('keeps the profile editor open and shows transform failures after a rejected file save', async () => {
    assert.ok(localUid && javascriptUid);
    const sourceUid = localUid;
    const transformUid = javascriptUid;
    requireCommitted(
      await setGlobalTransforms([]),
      'global chain reset before profile editor diagnostics',
    );
    await waitForGlobalChain([]);
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
    const failingFile = [
      'export default function () {',
      '  throw new Error("profile editor intentional failure");',
      '}',
      '',
    ].join('\n');
    await assert.rejects(
      invoke<MutationOutcome<null>>('save_profile_file', {
        uid: transformUid,
        fileData: failingFile,
      }),
    );
    assert.equal(
      await invoke<string>('read_profile_file', { uid: transformUid }),
      originalFile,
      'a rejected script candidate must preserve the previous file content',
    );

    const failure = await $('[data-slot="profile-editor-runtime-failure"]');
    await failure.waitForDisplayed({ timeout: 30_000 });
    assert.match(await failure.getText(), /profile editor intentional failure/);

    await browser.refresh();
    await browser.waitUntil(
      async () =>
        browser.execute(() =>
          Boolean(document.querySelector('[data-slot="profile-editor-save"]')),
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'Profile editor did not reload after the rejected save.',
      },
    );
    assert.match(
      await invoke<string>('read_profile_file', { uid: transformUid }),
      /chain ui javascript log/,
    );

    const refreshedFailure = await $(
      '[data-slot="profile-editor-runtime-failure"]',
    );
    await refreshedFailure.waitForDisplayed({ timeout: 30_000 });
    assert.match(
      await refreshedFailure.getText(),
      /profile editor intentional failure/,
    );
    assert.equal(
      (await browser.getWindowHandles()).includes(editorLabel),
      true,
      'rejected profile save unexpectedly closed the editor window',
    );

    const failed = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    const runtimeFailure = failed?.failure;
    if (!runtimeFailure) {
      throw new Error('runtime failure diagnostics were not available');
    }
    assert.ok(runtimeFailure.attempt_revision > before.revision);
    assert.equal(
      failed?.revision,
      before.revision,
      'failed file save must not replace the applied runtime revision',
    );
    assert.equal(runtimeFailure.transform_uid, transformUid);
    assert.equal(runtimeFailure.scope_uid, sourceUid);

    const repairedFile = [
      'export default function (config) {',
      '  console.info("profile editor intentional failure repaired");',
      '  return config;',
      '}',
      '',
    ].join('\n');
    requireApplied(
      await invoke<MutationOutcome<null>>('save_profile_file', {
        uid: transformUid,
        fileData: repairedFile,
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

    const trigger = await $('[data-slot="global-transform-chain-nav"]');
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
      .$('[data-slot="transform-chain-add"]')
      .click();
    await editor
      .$(
        `[data-slot="transform-chain-inactive-item"][data-profile-uid="${javascriptUid}"]`,
      )
      .$('[data-slot="transform-chain-add"]')
      .click();
    const originalOrder = [mergeAUid, javascriptUid];
    assert.deepEqual(await activeOrder('global'), originalOrder);

    await editor.$('[data-slot="transform-chain-reset"]').click();
    await waitForActiveOrder('global', []);
    await waitForGlobalChain([]);

    editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    for (const uid of originalOrder) {
      const item = await editor.$(
        `[data-slot="transform-chain-inactive-item"][data-profile-uid="${uid}"]`,
      );
      await item.$('[data-slot="transform-chain-add"]').click();
    }
    assert.deepEqual(await activeOrder('global'), originalOrder);

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
    await waitForApplySettled('global');

    let currentTrigger = await $('[data-slot="global-transform-chain-nav"]');
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
    await waitForApplySettled('global');

    currentTrigger = await $('[data-slot="global-transform-chain-nav"]');
    await currentTrigger.waitForClickable({ timeout: 15_000 });
    await currentTrigger.click();
    editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });
    javascriptRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${javascriptUid}"]`,
    );
    await javascriptRow.$('[data-slot="transform-chain-move-down"]').click();
    assert.deepEqual(await activeOrder('global'), [mergeAUid, javascriptUid]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid, javascriptUid]);
    const movedDown = await waitForGlobalRuntimeLog(
      javascriptUid,
      ['info', 'chain ui javascript log'],
      reordered.revision,
    );
    await waitForApplySettled('global');

    currentTrigger = await $('[data-slot="global-transform-chain-nav"]');
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
      movedDown.revision,
    );
    assert.ok(cleared.revision > movedDown.revision);
    await waitForApplySettled('global');

    currentTrigger = await $('[data-slot="global-transform-chain-nav"]');
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
  });

  // Contract: a failing JS transform candidate is rejected without changing
  // the global chain; diagnostics identify its UID in the editor draft, then a
  // valid chain update clears the failure.
  it('rejects a failed global transform candidate and pins diagnostics to its script', async () => {
    assert.ok(mergeAUid && failingJavascriptUid);
    requireApplied(
      await setGlobalTransforms([mergeAUid]),
      'global transform baseline before failed-script diagnostics',
    );
    await waitForGlobalChain([mergeAUid]);
    await openRoute('/main/profiles/merge');

    const before = await invoke<RuntimeTransformDiagnostics | null>(
      'get_runtime_transform_diagnostics',
    );
    assert.ok(before);
    assert.equal(before.failure, null);

    const trigger = await $('[data-slot="global-transform-chain-nav"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();
    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingTransform = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${failingJavascriptUid}"]`,
    );
    await failingTransform.$('[data-slot="transform-chain-add"]').click();
    assert.deepEqual(await activeOrder('global'), [
      mergeAUid,
      failingJavascriptUid,
    ]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid]);
    await editor.waitForDisplayed({ timeout: 15_000 });
    assert.deepEqual(await activeOrder('global'), [
      mergeAUid,
      failingJavascriptUid,
    ]);

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

    for (const transformUid of [failingJavascriptUid, mergeAUid]) {
      const currentRow = await editor.$(
        `[data-slot="transform-chain-active-item"][data-profile-uid="${transformUid}"]`,
      );
      await currentRow.$('[data-slot="transform-chain-remove"]').click();
    }
    await waitForActiveOrder('global', []);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([]);
    await waitForApplySettled('global');

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

  // Contract: a failing scoped transform candidate is rejected without
  // changing the Profile chain; diagnostics identify its UID and source in the
  // editor draft, then a valid chain update clears the failure.
  it('rejects a failed scoped transform candidate and pins diagnostics to its script', async () => {
    assert.ok(localUid && mergeAUid && failingJavascriptUid);
    requireCommitted(
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
    await failingTransform.$('[data-slot="transform-chain-add"]').click();
    assert.deepEqual(await activeOrder('profile'), [
      mergeAUid,
      failingJavascriptUid,
    ]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForScopedChain(localUid, [mergeAUid]);
    await editor.waitForDisplayed({ timeout: 15_000 });
    assert.deepEqual(await activeOrder('profile'), [
      mergeAUid,
      failingJavascriptUid,
    ]);

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
    const baselineRow = await editor.$(
      `[data-slot="transform-chain-active-item"][data-profile-uid="${mergeAUid}"]`,
    );
    await baselineRow.$('[data-slot="transform-chain-remove"]').click();
    await waitForActiveOrder('profile', []);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForScopedChain(localUid, []);
    await waitForApplySettled('profile');

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

  // Contract: editing an active script to fail is rejected and preserves the
  // file and runtime; the open editor shows the attempt, then valid content
  // applies and removes the failure row.
  it('rejects an invalid active script edit and refreshes diagnostics', async () => {
    assert.ok(localUid && javascriptUid);
    const sourceUid = localUid;
    const transformUid = javascriptUid;
    requireCommitted(
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

    const originalFile = await invoke<string>('read_profile_file', {
      uid: transformUid,
    });
    const failingFile = [
      'export default function () {',
      '  throw new Error("chain ui edited script failure");',
      '}',
      '',
    ].join('\n');
    await assert.rejects(
      invoke<MutationOutcome<null>>('save_profile_file', {
        uid: transformUid,
        fileData: failingFile,
      }),
    );
    assert.equal(
      await invoke<string>('read_profile_file', { uid: transformUid }),
      originalFile,
      'a rejected script candidate must preserve the previous file content',
    );

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

    const repairedFile = [
      'export default function (config) {',
      '  console.info("chain ui edited script repaired");',
      '  return config;',
      '}',
      '',
    ].join('\n');
    requireApplied(
      await invoke<MutationOutcome<null>>('save_profile_file', {
        uid: transformUid,
        fileData: repairedFile,
      }),
      'repair active JavaScript profile content',
    );
    assert.equal(
      await invoke<string>('read_profile_file', { uid: transformUid }),
      repairedFile,
      'valid repaired script content was not committed',
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
  });

  // Contract: selecting invalid overlay YAML is rejected without changing the
  // global chain; diagnostics identify the overlay in the editor draft, then a
  // valid chain update clears the failure.
  it('rejects an invalid merge candidate and pins diagnostics to its row', async () => {
    assert.ok(failingMergeUid && mergeAUid);
    requireCommitted(
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

    const trigger = await $('[data-slot="global-transform-chain-nav"]');
    await trigger.waitForClickable({ timeout: 15_000 });
    await trigger.click();
    const editor = await $(
      '[data-slot="transform-chain-editor"][data-chain-scope="global"]',
    );
    await editor.waitForDisplayed({ timeout: 15_000 });

    const failingTransform = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${failingMergeUid}"]`,
    );
    await failingTransform.$('[data-slot="transform-chain-add"]').click();
    assert.deepEqual(await activeOrder('global'), [failingMergeUid]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([]);
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
    // `data-transform-type` exposes the Profile type discriminator; the user
    // facing label identifies this Profile's transform implementation.
    assert.equal(
      await failure.getAttribute('data-transform-type'),
      'transform',
    );
    assert.equal(await failure.getAttribute('data-script-type'), null);
    assert.match(await failure.getText(), /Merge \(YAML\)/);
    assert.match(await failure.getText(), /not a mapping/);

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
    assert.match(failedDiagnostics.failure.message, /not a mapping/);

    await failingRow.$('[data-slot="transform-chain-remove"]').click();
    const validTransform = await editor.$(
      `[data-slot="transform-chain-inactive-item"][data-profile-uid="${mergeAUid}"]`,
    );
    await validTransform.$('[data-slot="transform-chain-add"]').click();
    assert.deepEqual(await activeOrder('global'), [mergeAUid]);
    await editor.$('[data-slot="transform-chain-save"]').click();
    await waitForGlobalChain([mergeAUid]);
    await waitForApplySettled('global');

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
