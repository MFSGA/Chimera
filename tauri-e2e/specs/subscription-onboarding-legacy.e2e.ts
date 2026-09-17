import assert from 'node:assert/strict';
import { createServer, type Server } from 'node:http';

/*
契约 ID：legacy-subscription-onboarding-layout-and-fallback
用例标题：打开 legacy 订阅导入向导并覆盖成功、代理选择及失败重试
风险/回归：页面把流程状态做成瞬时通知、漏掉某个阶段、默认失败后没有继续探测、失败后不能重试，或 legacy 路由没有挂载向导
边界：桌面 UI E2E（布局 + 本地确定性订阅服务的 fallback 主路径）
基础套件：profiles；本轮单独执行该 spec，未执行完整 profiles 套件
运行条件：Windows Tauri E2E、legacy 窗口、zh-cn
真实依赖：Tauri E2E 二进制、legacy WebView、TanStack Router 页面挂载、应用真实导入/探测 IPC
模拟与绕过：订阅服务是测试内 127.0.0.1 HTTP 服务；成功路径第一次导入返回无效订阅、后续返回有效 YAML，重试路径前六次请求返回无效订阅、第七次返回有效 YAML；代理/TUN 只在测试前确认关闭，不执行宿主设置变更
初始状态：切换到 legacy 窗口并打开 /subscription-onboarding；断言系统代理和 TUN 均关闭
被测操作：通过真实浏览器导航和 QuickImport 输入控件触发导入
权威结果：DOM 中的 data-slot 区域、每个区域的 data-status、成功结果，以及持久化的测试订阅
当前 UI 结果：布局测试确认五个步骤初始为“等待执行”；fallback 测试确认默认失败、地址检测、稳定性检测、直连导入依次完成；另验证成功后的代理选择和直连失败后的重试
磁盘/刷新/冷启动：不适用；本契约只验证首次页面装配
失败或边界场景：任一流程区域缺失时断言失败；网络行为和宿主设置不在本契约范围
等待预算：页面挂载最多 30 秒；本地 fallback 主路径最多 45 秒
诊断：失败由 WDIO 保存错误；设置 CHIMERA_E2E_ARTIFACT_DIR 时额外保存截图和 HTML，产物不含订阅凭据
资源与清理：关闭本地 HTTP 服务；删除本轮唯一名称的测试 profile，并恢复测试前的当前 profile、语言配置和语言缓存
回归敏感性：缺少任一固定流程区域、输入控件、成功后的代理选择、失败后的重试入口或误显示结果区域时断言失败
相关规则与例外：T01、T02、T04、T14；本地服务证明应用调用链，不证明公网网络、系统代理授权或 TUN 驱动行为
*/

const targetPath = '/subscription-onboarding';
const fixtureProfileName = `TDD Best Effort Subscription ${process.pid}-${Date.now()}`;
const subscriptionPath = '/subscription.yaml';
const retryPath = '/retry.yaml';
const invalidSubscription = 'mode: rule\n';
const validSubscription = 'mode: rule\nproxies: []\n';

type ProfilesResponse = {
  current: string | null;
  items: Array<{ name: string; uid: string }>;
};

type VergeConfig = {
  language?: string | null;
  enable_system_proxy?: boolean | null;
  enable_tun_mode?: boolean | null;
};

type CleanupAction = () => Promise<void>;

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

async function closeServer(server: Server | undefined) {
  if (!server?.listening) return;
  await new Promise<void>((resolve, reject) =>
    server.close((error) => (error ? reject(error) : resolve())),
  );
}

async function collectCleanupError(errors: unknown[], action: CleanupAction) {
  try {
    await action();
  } catch (error) {
    errors.push(error);
  }
}

describe('legacy subscription onboarding', () => {
  let server: Server | undefined;
  let serverPort: number | undefined;
  const requestCounts = new Map<string, number>();
  let initialCurrentProfile: string | null = null;
  let initialLanguage: string | null = null;
  let initialLanguageCache: string | null = null;

  before(async () => {
    await browser.switchToWindow('legacy');
    const initialVerge = await invoke<VergeConfig>('get_verge_config');
    initialLanguage = initialVerge.language ?? null;
    initialLanguageCache = await browser.execute(() =>
      localStorage.getItem(btoa('paraglide-language-cache')),
    );
    await invoke('patch_verge_config', {
      payload: { language: 'zh-cn' },
    });
    await browser.execute(() => {
      localStorage.setItem(btoa('paraglide-language-cache'), 'zh-cn');
    });
    await browser.refresh();

    const currentUrl = new URL(await browser.getUrl());
    currentUrl.pathname = targetPath;
    currentUrl.search = '';
    await browser.url(currentUrl.href);

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelector('[data-slot="best-effort-import-steps"]') !==
            null,
        ),
      {
        timeout: 30_000,
        timeoutMsg: 'The legacy subscription onboarding page did not render.',
      },
    );

    const verge = await invoke<VergeConfig>('get_verge_config');
    assert.equal(
      Boolean(verge.enable_system_proxy),
      false,
      'The fallback E2E must start with system proxy disabled.',
    );
    assert.equal(
      Boolean(verge.enable_tun_mode),
      false,
      'The fallback E2E must start with TUN disabled.',
    );

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    initialCurrentProfile = profiles.current;

    server = createServer((request, response) => {
      const requestPath = new URL(request.url ?? '/', 'http://127.0.0.1')
        .pathname;
      const requestCount = (requestCounts.get(requestPath) ?? 0) + 1;
      requestCounts.set(requestPath, requestCount);
      const knownPath =
        requestPath === subscriptionPath || requestPath === retryPath;
      const body =
        requestPath === subscriptionPath
          ? requestCount === 1
            ? invalidSubscription
            : validSubscription
          : requestPath === retryPath && requestCount <= 6
            ? invalidSubscription
            : validSubscription;
      response.writeHead(knownPath ? 200 : 404, {
        'content-type': 'application/yaml',
        'content-length': Buffer.byteLength(body),
        'profile-title': fixtureProfileName,
      });
      response.end(body);
    });
    await new Promise<void>((resolve, reject) => {
      server?.once('error', reject);
      server?.listen(0, '127.0.0.1', resolve);
    });
    const address = server.address();
    assert.ok(address && typeof address !== 'string');
    serverPort = address.port;
  });

  after(async () => {
    const cleanupErrors: unknown[] = [];
    await collectCleanupError(cleanupErrors, () => closeServer(server));

    let profiles: ProfilesResponse | null = null;
    try {
      profiles = await invoke<ProfilesResponse>('get_profiles');
    } catch (error) {
      cleanupErrors.push(error);
    }
    if (profiles) {
      await collectCleanupError(cleanupErrors, () =>
        invoke('activate_profile', { uid: null }),
      );
      for (const item of profiles.items.filter(
        (candidate) => candidate.name === fixtureProfileName,
      )) {
        await collectCleanupError(cleanupErrors, () =>
          invoke('delete_profile', { uid: item.uid }),
        );
      }
      if (
        initialCurrentProfile &&
        profiles.items.some((item) => item.uid === initialCurrentProfile)
      ) {
        await collectCleanupError(cleanupErrors, () =>
          invoke('activate_profile', {
            uid: initialCurrentProfile,
          }),
        );
      }
    }

    if (initialLanguage) {
      await collectCleanupError(cleanupErrors, () =>
        invoke('patch_verge_config', {
          payload: { language: initialLanguage },
        }),
      );
    }

    await collectCleanupError(cleanupErrors, () =>
      browser.execute((value) => {
        const key = btoa('paraglide-language-cache');
        if (value === null) {
          localStorage.removeItem(key);
        } else {
          localStorage.setItem(key, value);
        }
      }, initialLanguageCache),
    );

    await collectCleanupError(cleanupErrors, async () => {
      const restoredProfiles = await invoke<ProfilesResponse>('get_profiles');
      if (
        restoredProfiles.current !== initialCurrentProfile ||
        restoredProfiles.items.some((item) => item.name === fixtureProfileName)
      ) {
        throw new Error(
          'The test profile or current profile was not restored.',
        );
      }
    });
    if (initialLanguage) {
      await collectCleanupError(cleanupErrors, async () => {
        const restoredVerge = await invoke<VergeConfig>('get_verge_config');
        if (restoredVerge.language !== initialLanguage) {
          throw new Error('The test language configuration was not restored.');
        }
      });
    }
    await collectCleanupError(cleanupErrors, async () => {
      const restoredLanguageCache = await browser.execute(() =>
        localStorage.getItem(btoa('paraglide-language-cache')),
      );
      if (restoredLanguageCache !== initialLanguageCache) {
        throw new Error('The test language cache was not restored.');
      }
    });

    if (cleanupErrors.length) {
      throw new AggregateError(
        cleanupErrors,
        'Legacy subscription onboarding cleanup failed.',
      );
    }
  });

  it('keeps every import stage visible before execution', async () => {
    const state = await browser.execute(() => {
      const stepSelectors = [
        'default-import',
        'network-setup',
        'address-check',
        'stability-check',
        'direct-import',
      ].map((id) => `[data-slot="best-effort-step-${id}"]`);

      return {
        input:
          document.querySelector('[data-slot="quick-import-input"] input') !==
          null,
        steps: stepSelectors.map((selector) => {
          const element = document.querySelector<HTMLElement>(selector);
          return {
            exists: element !== null,
            text: element?.innerText ?? '',
          };
        }),
        result: document.querySelector(
          '[data-slot="best-effort-import-result"]',
        ),
      };
    });

    assert.equal(state.input, true, JSON.stringify(state, null, 2));
    assert.equal(state.steps.length, 5);
    assert.equal(
      state.steps.every(
        (step) => step.exists && step.text.includes('等待执行'),
      ),
      true,
      JSON.stringify(state, null, 2),
    );
    assert.equal(state.result, null);
  });

  it('continues through address and stability checks after default import fails', async () => {
    assert.ok(serverPort);

    const input = await $('[data-slot="quick-import-input"] input');
    await input.setValue(`http://127.0.0.1:${serverPort}${subscriptionPath}`);
    await browser.keys('Enter');

    await browser.waitUntil(
      async () =>
        browser.execute(() => {
          const result = document.querySelector<HTMLElement>(
            '[data-slot="best-effort-import-result"]',
          );
          return result?.innerText.includes('订阅已成功导入') ?? false;
        }),
      {
        timeout: 45_000,
        timeoutMsg: 'The local fallback subscription import did not succeed.',
      },
    );

    const state = await browser.execute(() => {
      const ids = [
        'default-import',
        'network-setup',
        'address-check',
        'stability-check',
        'direct-import',
      ];
      return ids.map((id) => {
        const element = document.querySelector<HTMLElement>(
          `[data-slot="best-effort-step-${id}"]`,
        );
        return {
          id,
          status: element?.dataset.status,
          text: element?.innerText ?? '',
        };
      });
    });

    assert.deepEqual(
      state.map((step) => step.status),
      ['failure', 'success', 'success', 'success', 'success'],
      JSON.stringify(state, null, 2),
    );
    assert.match(state[0].text, /失败/);
    assert.match(state[3].text, /已完成/);
    assert.match(state[4].text, /已完成/);
    assert.equal(
      requestCounts.get(subscriptionPath),
      6,
      JSON.stringify(state, null, 2),
    );

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelector('[data-slot="best-effort-skip-proxy"]') !==
            null,
        ),
      {
        timeout: 5_000,
        timeoutMsg: 'The proxy decision actions did not render.',
      },
    );
    const skipProxy = await $('[data-slot="best-effort-skip-proxy"]');
    assert.equal(await skipProxy.isDisplayed(), true);
    await skipProxy.click();
    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document
              .querySelector<HTMLElement>(
                '[data-slot="best-effort-import-result"]',
              )
              ?.innerText.includes('系统代理保持关闭') ?? false,
        ),
      {
        timeout: 5_000,
        timeoutMsg: 'The proxy skip decision was not reflected in the result.',
      },
    );

    const profiles = await invoke<ProfilesResponse>('get_profiles');
    assert.ok(
      profiles.items.some((item) => item.name === fixtureProfileName),
      'The fallback subscription was not persisted as a profile.',
    );
  });

  it('reports direct failure and recovers through the retry action', async () => {
    assert.ok(serverPort);

    const input = await $('[data-slot="quick-import-input"] input');
    await input.setValue(`http://127.0.0.1:${serverPort}${retryPath}`);
    await browser.keys('Enter');

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document
              .querySelector<HTMLElement>(
                '[data-slot="best-effort-import-result"]',
              )
              ?.innerText.includes('尽力了，目前仍无法完成订阅导入') ?? false,
        ),
      {
        timeout: 45_000,
        timeoutMsg: 'The direct import failure result did not render.',
      },
    );

    const failedState = await browser.execute(() => {
      const ids = [
        'default-import',
        'network-setup',
        'address-check',
        'stability-check',
        'direct-import',
      ];
      return ids.map(
        (id) =>
          document.querySelector<HTMLElement>(
            `[data-slot="best-effort-step-${id}"]`,
          )?.dataset.status,
      );
    });
    assert.deepEqual(failedState, [
      'failure',
      'success',
      'success',
      'success',
      'failure',
    ]);

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document.querySelector('[data-slot="best-effort-retry"]') !== null,
        ),
      {
        timeout: 5_000,
        timeoutMsg: 'The retry action did not render.',
      },
    );
    const retry = await $('[data-slot="best-effort-retry"]');
    assert.equal(await retry.isDisplayed(), true);
    await retry.click();

    await browser.waitUntil(
      async () =>
        browser.execute(
          () =>
            document
              .querySelector<HTMLElement>(
                '[data-slot="best-effort-import-result"]',
              )
              ?.innerText.includes('订阅已成功导入') ?? false,
        ),
      {
        timeout: 45_000,
        timeoutMsg: 'The retry action did not recover the subscription import.',
      },
    );

    const recoveredState = await browser.execute(() => {
      const ids = [
        'default-import',
        'network-setup',
        'address-check',
        'stability-check',
        'direct-import',
      ];
      return ids.map(
        (id) =>
          document.querySelector<HTMLElement>(
            `[data-slot="best-effort-step-${id}"]`,
          )?.dataset.status,
      );
    });
    assert.deepEqual(recoveredState, [
      'success',
      'skipped',
      'skipped',
      'skipped',
      'skipped',
    ]);
    assert.equal(requestCounts.get(retryPath), 7);
  });
});
