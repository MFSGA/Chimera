import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { seedE2eDisabledAutoUpdate } from './e2e-update-preference.ts';

// Contract: a fresh E2E runtime starts with automatic app update checks off,
// without creating an incomplete typed configuration or changing production code.
test('fresh E2E runtime seeds only the legacy update preference', () => {
  const config = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-e2e-update-'));
  try {
    seedE2eDisabledAutoUpdate(config);
    assert.equal(
      fs.readFileSync(path.join(config, 'chimera-config.yaml'), 'utf8'),
      'enable_auto_check_update: false\n',
    );
    assert.equal(fs.existsSync(path.join(config, 'application.yaml')), false);
  } finally {
    fs.rmSync(config, { recursive: true, force: true });
  }
});

// Contract: explicit test fixtures retain unrelated startup settings.
test('existing E2E fixture is preserved and its update setting is disabled once', () => {
  const config = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-e2e-update-'));
  const legacy = path.join(config, 'chimera-config.yaml');
  try {
    fs.writeFileSync(
      legacy,
      'window_type: main\nenable_auto_check_update: true\n',
    );
    seedE2eDisabledAutoUpdate(config);
    seedE2eDisabledAutoUpdate(config);
    assert.equal(
      fs.readFileSync(legacy, 'utf8'),
      'window_type: main\nenable_auto_check_update: false\n',
    );
  } finally {
    fs.rmSync(config, { recursive: true, force: true });
  }
});

// Contract: a reused/upgrade runtime's already-migrated state is not modified.
test('existing typed application state is never overwritten by the E2E seed', () => {
  const config = fs.mkdtempSync(path.join(os.tmpdir(), 'chimera-e2e-update-'));
  const typed = path.join(config, 'application.yaml');
  try {
    fs.writeFileSync(typed, 'enable_auto_check_update: true\n');
    seedE2eDisabledAutoUpdate(config);
    assert.equal(
      fs.readFileSync(typed, 'utf8'),
      'enable_auto_check_update: true\n',
    );
    assert.equal(
      fs.existsSync(path.join(config, 'chimera-config.yaml')),
      false,
    );
  } finally {
    fs.rmSync(config, { recursive: true, force: true });
  }
});
