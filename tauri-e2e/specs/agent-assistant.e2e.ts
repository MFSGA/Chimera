import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { displayedElement } from './interaction.js';
import { openMainRoute } from './main-window.js';

const targetPath = '/main/assistant';
const artifactDirectory = path.resolve('.tmp');

type VergeConfig = {
  language?: string | null;
};

/** Invoke a backend command through the current Tauri page internals. */
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

/** Navigate through the real Help menu so Agent discoverability stays covered. */
async function openAgentFromHelp() {
  const helpButton = await displayedElement('[data-slot="header-help-menu"]');
  await browser.execute((button) => {
    button.dispatchEvent(
      new PointerEvent('pointerdown', {
        bubbles: true,
        button: 0,
        pointerType: 'mouse',
      }),
    );
  }, helpButton);

  const link = await displayedElement('a[href="/main/assistant"]');
  await browser.execute((element) => (element as HTMLElement).click(), link);
  await browser.waitUntil(
    async () =>
      browser.execute((expected) => location.pathname === expected, targetPath),
    { timeout: 15_000, timeoutMsg: 'Agent route did not open.' },
  );
  await $('[data-slot="agent-page-scroll-area"]').waitForDisplayed({
    timeout: 15_000,
  });
}

/** Return stable viewport metrics used by desktop and narrow-window assertions. */
async function getLayoutState() {
  return browser.execute(() => {
    const root = document.querySelector<HTMLElement>(
      '[data-slot="agent-page-scroll-area"]',
    );
    return {
      viewportWidth: window.innerWidth,
      documentWidth: document.documentElement.scrollWidth,
      rootWidth: root ? Math.round(root.getBoundingClientRect().width) : 0,
    };
  });
}

describe('network assistant guided diagnosis', () => {
  let originalLanguage: string | undefined;

  before(async () => {
    fs.mkdirSync(artifactDirectory, { recursive: true });
    originalLanguage =
      (await invoke<VergeConfig>('get_verge_config')).language ?? undefined;
    await invoke('patch_verge_config', {
      payload: { language: 'zh-cn' },
    });
    await openMainRoute('/main/dashboard');
    await browser.setWindowSize(1240, 720);
    await openAgentFromHelp();
  });

  after(async () => {
    if (originalLanguage === undefined) return;
    await invoke('patch_verge_config', {
      payload: { language: originalLanguage },
    }).catch(() => undefined);
  });

  it('starts with a clear read-only guided action', async () => {
    const body = await $('body');
    await body.waitForDisplayed({ timeout: 15_000 });
    const text = await body.getText();

    assert.match(text, /遇到连接或代理问题/);
    assert.match(text, /检查不会修改任何设置/);
    assert.match(text, /任何网络变更都一定会先征得你的确认/);

    const button = await displayedElement(
      '//button[contains(., "检查网络问题")]',
    );
    const enabled = await browser.execute(
      (element) => !(element as HTMLButtonElement).disabled,
      button,
    );
    assert.equal(enabled, true);
  });

  it('turns a finding into an explanation and recommended repair', async () => {
    const button = await displayedElement(
      '//button[contains(., "检查网络问题")]',
    );
    await browser.execute(
      (element) => (element as HTMLElement).click(),
      button,
    );

    await browser.waitUntil(
      async () =>
        (await $('body').getText()).includes('系统代理可能正在阻断网络连接'),
      { timeout: 15_000, timeoutMsg: 'Recommended finding did not render.' },
    );

    const text = await $('body').getText();
    assert.match(text, /检查结果/);
    assert.match(text, /发现 1 个需要关注的问题/);
    assert.match(text, /系统代理仍在把流量发送给 Chimera/);
    assert.match(text, /推荐修复/);
    assert.match(text, /查看修复方案/);

    const details = await $('details');
    assert.equal(await details.getAttribute('open'), null);
    await browser.saveScreenshot(
      path.join(artifactDirectory, 'agent-desktop.png'),
    );
  });

  it('keeps technical details secondary and fits a narrow window', async () => {
    const summary = await displayedElement(
      '//summary[contains(., "技术详情")]',
    );
    await browser.execute(
      (element) => (element as HTMLElement).click(),
      summary,
    );
    assert.match(await $('body').getText(), /高级手动控制/);

    await browser.setWindowSize(680, 720);
    const layout = await getLayoutState();
    assert.ok(layout.viewportWidth <= 700, JSON.stringify(layout));
    assert.ok(
      layout.documentWidth <= layout.viewportWidth + 1,
      JSON.stringify(layout),
    );
    assert.ok(layout.rootWidth <= layout.viewportWidth, JSON.stringify(layout));

    const repair = await $('//button[contains(., "查看修复方案")]');
    await repair.scrollIntoView();
    assert.equal(await repair.isDisplayed(), true);
    await browser.saveScreenshot(
      path.join(artifactDirectory, 'agent-narrow.png'),
    );
  });

  it('requires confirmation and shows the verified healthy result after execute', async () => {
    const repair = await displayedElement(
      '//button[contains(., "查看修复方案")]',
    );
    await browser.execute(
      (element) => (element as HTMLElement).click(),
      repair,
    );

    await browser.waitUntil(
      async () => (await $('body').getText()).includes('确认网络变更'),
      { timeout: 15_000, timeoutMsg: 'Confirmation dialog did not render.' },
    );
    const dialogText = await $('body').getText();
    assert.match(dialogText, /主机网络设置将发生变化/);
    assert.match(dialogText, /将禁用主机的系统代理/);
    assert.match(dialogText, /确认并执行/);

    const confirm = await displayedElement(
      '//button[contains(., "确认并执行")]',
    );
    await browser.execute(
      (element) => (element as HTMLElement).click(),
      confirm,
    );

    await browser.waitUntil(
      async () => (await $('body').getText()).includes('当前看起来一切正常'),
      {
        timeout: 15_000,
        timeoutMsg: 'Verified healthy result did not render.',
      },
    );
    const finalText = await $('body').getText();
    assert.match(finalText, /这次检查没有发现已知的网络状态问题/);
    assert.doesNotMatch(finalText, /查看修复方案/);
    await browser.waitUntil(
      async () => !(await $('body').getText()).includes('确认网络变更'),
      { timeout: 15_000, timeoutMsg: 'Confirmation dialog did not close.' },
    );
  });
});
