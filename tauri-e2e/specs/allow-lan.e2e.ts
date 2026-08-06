import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { createServer, type Server } from 'node:http';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);
const wslDistribution = process.env.CHIMERA_E2E_WSL_DISTRO ?? 'Ubuntu-24.04';
const lanSshHost = process.env.CHIMERA_E2E_LAN_SSH_HOST;
const requestedCore = process.env.CHIMERA_E2E_CORE;

interface ClashRuntimeState {
  allowLan: boolean;
  mixedPort: number;
}

async function readClashRuntimeState(): Promise<ClashRuntimeState> {
  const info = await browser.execute(async () => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string) => Promise<{
            secret?: string;
            server: string;
          }>;
        };
      }
    ).__TAURI_INTERNALS__;
    return tauri.invoke('get_clash_info');
  });
  const response = await fetch(`http://${info.server}/configs`, {
    headers: info.secret
      ? { Authorization: `Bearer ${info.secret}` }
      : undefined,
  });
  if (!response.ok) {
    throw new Error(`Clash config query failed: ${response.status}`);
  }

  const config = (await response.json()) as {
    'allow-lan': boolean;
    'mixed-port': number;
  };
  return {
    allowLan: config['allow-lan'],
    mixedPort: config['mixed-port'],
  };
}

async function readAllowLanSwitch(): Promise<boolean | null> {
  return browser.execute(() => {
    const labels = [
      'Allow LAN',
      'å…è®¸å±€åŸŸç½‘è¿žæŽ¥',
      'å…è¨±å€åŸŸç¶²è·¯é€£ç·š',
      'Ð Ð°Ð·Ñ€ÐµÑˆÐ¸Ñ‚ÑŒ LAN',
    ];
    void labels;
    const row = Array.from(document.querySelectorAll<HTMLElement>('li')).find(
      (element) => {
        const text = element.textContent?.replace(/\s+/g, ' ').trim() ?? '';
        return /allow\s*lan|lan|局域网|區域網/i.test(text);
      },
    );
    const input = row?.querySelector<HTMLInputElement>(
      'input[type="checkbox"]',
    );
    return input?.checked ?? null;
  });
}

async function setAllowLan(enabled: boolean): Promise<void> {
  const current = await readClashRuntimeState();
  const currentUi = await readAllowLanSwitch();
  if (current.allowLan === enabled && currentUi === enabled) return;

  // The running core is authoritative. If only the already-mounted page is
  // stale, reload the settings page instead of clicking the switch again and
  // accidentally toggling the core to the opposite value.
  if (current.allowLan === enabled && currentUi !== enabled) {
    await browser.refresh();
    await browser.waitUntil(
      async () => (await readAllowLanSwitch()) === enabled,
      {
        timeout: 15_000,
        timeoutMsg: `Allow LAN UI did not become ${enabled} after refresh.`,
      },
    );
    return;
  }

  const clicked = await browser.execute(() => {
    const labels = [
      'Allow LAN',
      '允许局域网连接',
      '允許區域網路連線',
      'Разрешить LAN',
    ];
    void labels;
    const row = Array.from(document.querySelectorAll<HTMLElement>('li')).find(
      (element) => {
        const text = element.textContent?.replace(/\s+/g, ' ').trim() ?? '';
        return /allow\s*lan|lan|局域网|區域網/i.test(text);
      },
    );
    const input = row?.querySelector<HTMLInputElement>(
      'input[type="checkbox"]',
    );
    input?.click();
    return Boolean(input);
  });
  assert.equal(clicked, true, 'The Allow LAN switch was not found.');

  let lastState = 'unknown';
  await browser.waitUntil(
    async () => {
      const runtime = await readClashRuntimeState();
      const ui = await readAllowLanSwitch();
      lastState = `runtime=${runtime.allowLan}, ui=${String(ui)}`;
      return runtime.allowLan === enabled && ui === enabled;
    },
    {
      timeout: 15_000,
      timeoutMsg: `Allow LAN UI/core state did not become ${enabled} (${lastState}).`,
    },
  );
}

async function selectRequestedCore(): Promise<void> {
  if (!requestedCore) return;
  await browser.executeAsync((core, done) => {
    const tauri = (
      window as typeof window & {
        __TAURI_INTERNALS__: {
          invoke: (command: string, args: unknown) => Promise<unknown>;
        };
      }
    ).__TAURI_INTERNALS__;
    tauri
      .invoke('change_clash_core', { clashCore: core })
      .then(() => done())
      .catch((error) => done(error));
  }, requestedCore);
}

async function resolveWindowsLanAddress(): Promise<string> {
  const script = [
    '$address = Get-NetIPConfiguration |',
    '  Where-Object { $_.IPv4DefaultGateway -ne $null } |',
    '  ForEach-Object { $_.IPv4Address.IPAddress } |',
    '  Where-Object { $_ -and -not $_.StartsWith("169.254.") } |',
    '  Select-Object -First 1',
    'if (-not $address) { throw "No active LAN IPv4 address was found." }',
    '[Console]::Out.Write($address)',
  ].join('\n');
  const { stdout } = await execFileAsync('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    script,
  ]);
  const address = stdout.trim();
  assert.match(address, /^\d{1,3}(?:\.\d{1,3}){3}$/);
  return address;
}

async function startProbeServer(token: string): Promise<{
  port: number;
  server: Server;
}> {
  const server = createServer((request, response) => {
    if (request.url === `/${token}`) {
      response.writeHead(200, { 'content-type': 'text/plain' });
      response.end(token);
      return;
    }
    response.writeHead(404).end();
  });
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  return { port: address.port, server };
}

async function probeFromLanClient(
  host: string,
  proxyPort: number,
  targetPort: number,
  token: string,
): Promise<{ ok: boolean; output: string }> {
  try {
    const { stdout } = await execFileAsync(
      'wsl.exe',
      [
        '-d',
        wslDistribution,
        '--',
        ...(lanSshHost ? ['ssh', lanSshHost, 'curl'] : ['curl']),
        '--silent',
        '--show-error',
        '--fail',
        '--max-time',
        '5',
        '--proxy',
        `http://${host}:${proxyPort}`,
        `http://127.0.0.1:${targetPort}/${token}`,
      ],
      { timeout: 10_000 },
    );
    return { ok: stdout.trim() === token, output: stdout.trim() };
  } catch (error) {
    const detail = error as Error & { stderr?: string; stdout?: string };
    return {
      ok: false,
      output: [detail.message, detail.stderr, detail.stdout]
        .filter(Boolean)
        .join('\n')
        .trim(),
    };
  }
}

async function readListeningAddresses(port: number): Promise<string[]> {
  const script = [
    `Get-NetTCPConnection -State Listen -LocalPort ${port} -ErrorAction SilentlyContinue |`,
    '  Select-Object -ExpandProperty LocalAddress -Unique |',
    '  ConvertTo-Json -Compress',
  ].join('\n');
  const { stdout } = await execFileAsync('powershell.exe', [
    '-NoProfile',
    '-NonInteractive',
    '-Command',
    script,
  ]);
  if (!stdout.trim()) return [];
  const value = JSON.parse(stdout) as string | string[];
  return Array.isArray(value) ? value : [value];
}

describe('Chimera Allow LAN networking', () => {
  it('accepts proxy traffic from a LAN client only while Allow LAN is enabled', async () => {
    const currentHref = await browser.getUrl();
    await selectRequestedCore();
    if (requestedCore) {
      await browser.refresh();
    }
    await browser.waitUntil(
      async () => (await readClashRuntimeState()).mixedPort > 0,
      { timeout: 30_000, timeoutMsg: 'Clash runtime did not become ready.' },
    );
    await browser.url(new URL('/main/settings/clash', currentHref).href);

    const original = await readClashRuntimeState();
    const host = await resolveWindowsLanAddress();
    const token = `chimera-e2e-${Date.now()}`;
    const { port: targetPort, server } = await startProbeServer(token);

    try {
      // Establish the baseline through the same frontend control under test.
      await setAllowLan(false);
      await setAllowLan(true);
      assert.equal(
        await readAllowLanSwitch(),
        true,
        'The Allow LAN switch did not show enabled immediately after clicking.',
      );

      await browser.refresh();
      await browser.waitUntil(
        async () => {
          const runtime = await readClashRuntimeState();
          const ui = await readAllowLanSwitch();
          return runtime.allowLan === true && ui === true;
        },
        {
          timeout: 15_000,
          timeoutMsg:
            'Allow LAN was not still enabled in the frontend and runtime after refresh.',
        },
      );

      const allowed = await probeFromLanClient(
        host,
        original.mixedPort,
        targetPort,
        token,
      );
      const enabledListeners = await readListeningAddresses(original.mixedPort);
      assert.equal(
        allowed.ok,
        true,
        `The LAN proxy request failed after Allow LAN was enabled. Host: ${host}:${original.mixedPort}. Listening addresses: ${enabledListeners.join(', ') || 'none'}. Curl output: ${allowed.output}`,
      );

      await setAllowLan(false);
      const blocked = await probeFromLanClient(
        host,
        original.mixedPort,
        targetPort,
        token,
      );
      const disabledListeners = await readListeningAddresses(
        original.mixedPort,
      );
      assert.equal(
        blocked.ok,
        false,
        `The LAN client reached the proxy after Allow LAN was disabled. Listening addresses: ${disabledListeners.join(', ') || 'none'}`,
      );
    } finally {
      await setAllowLan(original.allowLan);
      await new Promise<void>((resolve, reject) =>
        server.close((error) => (error ? reject(error) : resolve())),
      );
    }
  });
});
