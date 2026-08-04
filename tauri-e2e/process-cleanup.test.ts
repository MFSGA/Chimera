import assert from 'node:assert/strict';
import path from 'node:path';
import test from 'node:test';
import { buildWindowsCleanupScript } from './process-cleanup.ts';

test('cleanup script is scoped to E2E binaries and runtime paths', () => {
  const binaryDirectory = path.resolve('backend/target/e2e/debug');
  const runtimeDirectory = path.resolve('tauri-e2e/.tmp/runtime');

  const script = buildWindowsCleanupScript(binaryDirectory, runtimeDirectory);

  assert.match(script, /chimera\.exe/);
  assert.match(script, /mihomo\.exe/);
  assert.match(script, /msedgedriver\.exe/);
  assert.ok(script.includes(`${binaryDirectory}${path.sep}`));
  assert.ok(script.includes(runtimeDirectory));
  assert.match(script, /ExecutablePath -like "\$binaryRoot\*"/);
  assert.match(script, /CommandLine -like "\*\$runtimeRoot\*"/);
  assert.doesNotMatch(script, /Stop-Process -Name/);
});

test('cleanup script safely quotes apostrophes in paths', () => {
  const script = buildWindowsCleanupScript(
    "C:\\work\\O'Brien\\debug",
    "C:\\work\\O'Brien\\runtime",
  );

  assert.ok(script.includes("O''Brien"));
  assert.doesNotMatch(script, /O'Brien/);
});
