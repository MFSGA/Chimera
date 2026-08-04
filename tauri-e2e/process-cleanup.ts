import { execFileSync } from 'node:child_process';
import path from 'node:path';

const powershellQuote = (value: string) => `'${value.replaceAll("'", "''")}'`;

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
