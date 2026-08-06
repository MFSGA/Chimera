import { execFileSync } from 'node:child_process';
import path from 'node:path';

const powershellQuote = (value: string) => `'${value.replaceAll("'", "''")}'`;
const internetSettingsPath =
  'HKCU:\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings';

export interface WindowsProxySnapshot {
  proxyEnable: number | null;
  proxyServer: string | null;
  proxyOverride: string | null;
  autoConfigUrl: string | null;
}

/** Capture host proxy settings before the production startup path mutates them. */
export function captureWindowsProxySettings(): WindowsProxySnapshot | null {
  if (process.platform !== 'win32') return null;

  const script = [
    `$settings = Get-ItemProperty ${powershellQuote(internetSettingsPath)}`,
    '[ordered]@{',
    '  proxyEnable = $settings.ProxyEnable',
    '  proxyServer = $settings.ProxyServer',
    '  proxyOverride = $settings.ProxyOverride',
    '  autoConfigUrl = $settings.AutoConfigURL',
    '} | ConvertTo-Json -Compress',
  ].join('\n');

  return JSON.parse(
    execFileSync(
      'powershell.exe',
      ['-NoProfile', '-NonInteractive', '-Command', script],
      { encoding: 'utf8' },
    ),
  ) as WindowsProxySnapshot;
}

/** Restore the exact proxy values present before the E2E desktop process ran. */
export function restoreWindowsProxySettings(
  snapshot: WindowsProxySnapshot | null,
): void {
  if (!snapshot || process.platform !== 'win32') return;

  const values = [
    ['ProxyEnable', snapshot.proxyEnable, 'DWord'],
    ['ProxyServer', snapshot.proxyServer, 'String'],
    ['ProxyOverride', snapshot.proxyOverride, 'String'],
    ['AutoConfigURL', snapshot.autoConfigUrl, 'String'],
  ] as const;
  const mutations = values.flatMap(([name, value, propertyType]) =>
    value === null
      ? [
          `if ($settings.PSObject.Properties.Name -contains ${powershellQuote(name)}) { Remove-ItemProperty -LiteralPath ${powershellQuote(internetSettingsPath)} -Name ${powershellQuote(name)} }`,
        ]
      : [
          `New-ItemProperty -LiteralPath ${powershellQuote(internetSettingsPath)} -Name ${powershellQuote(name)} -Value ${typeof value === 'string' ? powershellQuote(value) : value} -PropertyType ${propertyType} -Force | Out-Null`,
        ],
  );
  const script = [
    "$ErrorActionPreference = 'Stop'",
    `$settings = Get-ItemProperty -LiteralPath ${powershellQuote(internetSettingsPath)}`,
    ...mutations,
  ];

  execFileSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-Command', script.join('\n')],
    { stdio: 'ignore' },
  );
}

export function buildWindowsCleanupScript(
  binaryDirectory: string,
  runtimeRootDirectory: string,
): string {
  const binaryRoot = `${path.resolve(binaryDirectory)}${path.sep}`;
  const runtimeRoot = path.resolve(runtimeRootDirectory);

  return [
    `$binaryRoot = ${powershellQuote(binaryRoot)}`,
    `$runtimeRoot = ${powershellQuote(runtimeRoot)}`,
    '$targets = Get-CimInstance Win32_Process | Where-Object {',
    '  (($_.Name -in @("chimera.exe", "mihomo.exe")) -and ($_.ExecutablePath -like "$binaryRoot*")) -or',
    '  (($_.Name -eq "msedgedriver.exe") -and ($_.CommandLine -like "*$runtimeRoot*"))',
    '}',
    'foreach ($process in $targets) {',
    '  Stop-Process -Id $process.ProcessId -Force -ErrorAction SilentlyContinue',
    '}',
    '[Console]::Out.Write($targets.Count)',
  ].join('\n');
}

export async function cleanupE2eProcesses(
  binaryDirectory: string,
  runtimeRootDirectory: string,
): Promise<number> {
  if (process.platform !== 'win32') return 0;

  // The Tauri service and browser driver finish concurrently with onComplete.
  // A short retry window ensures independently spawned core processes are reaped.
  let cleaned = 0;
  for (let attempt = 0; attempt < 4; attempt += 1) {
    if (attempt > 0) {
      await new Promise((resolve) => setTimeout(resolve, 500));
    }
    const output = execFileSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        buildWindowsCleanupScript(binaryDirectory, runtimeRootDirectory),
      ],
      { encoding: 'utf8' },
    ).trim();
    cleaned += Number.parseInt(output, 10) || 0;
  }
  return cleaned;
}
