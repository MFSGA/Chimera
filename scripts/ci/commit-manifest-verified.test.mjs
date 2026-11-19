import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const script = join(
  dirname(fileURLToPath(import.meta.url)),
  'commit-manifest-verified.sh',
);
const verifiedResponse = JSON.stringify({
  data: {
    createCommitOnBranch: {
      commit: {
        oid: '0123456789abcdef0123456789abcdef01234567',
        url: 'https://github.com/MFSGA/Chimera/commit/0123456789abcdef0123456789abcdef01234567',
        signature: { isValid: true },
      },
    },
  },
});

function git(cwd, args) {
  return execFileSync('git', args, { cwd, encoding: 'utf8' }).trim();
}

function fixture(branch = 'master') {
  const dir = mkdtempSync(join(tmpdir(), 'chimera-verified-manifest-'));
  const manifest = join(dir, 'manifest/version.json');
  const capture = join(dir, 'gh-arguments.txt');
  const executable = join(dir, 'bin/gh');
  mkdirSync(dirname(manifest), { recursive: true });
  mkdirSync(dirname(executable), { recursive: true });
  git(dir, ['init', '-q', '-b', branch]);
  writeFileSync(manifest, '{"version":"initial"}\n');
  git(dir, ['add', '--', 'manifest/version.json']);
  git(dir, [
    '-c',
    'user.name=Test',
    '-c',
    'user.email=test@example.invalid',
    '-c',
    'commit.gpgsign=false',
    'commit',
    '-qm',
    'test: base',
  ]);
  writeFileSync(
    executable,
    '#!/bin/sh\nprintf "%s\\0" "$@" > "$CAPTURE_FILE"\n' +
      'if [ "${MOCK_EXIT_CODE:-0}" != 0 ]; then exit "$MOCK_EXIT_CODE"; fi\n' +
      'printf "%s\\n" "$MOCK_RESPONSE"\n',
    { mode: 0o755 },
  );
  return {
    dir,
    manifest,
    capture,
    run({
      target = branch,
      mockResponse = verifiedResponse,
      mockExitCode = '0',
    } = {}) {
      return spawnSync('bash', [script, target], {
        cwd: dir,
        encoding: 'utf8',
        env: {
          ...process.env,
          PATH: `${join(dir, 'bin')}:${process.env.PATH}`,
          GH_TOKEN: 'test-token',
          GITHUB_REPOSITORY: 'MFSGA/Chimera',
          CAPTURE_FILE: capture,
          MOCK_RESPONSE: mockResponse,
          MOCK_EXIT_CODE: mockExitCode,
        },
      });
    },
    cleanup() {
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

test('commits only manifest bytes through verified GraphQL mutation and current head fence', () => {
  const f = fixture();
  try {
    const changed = '{"version":"updated"}\n';
    writeFileSync(f.manifest, changed);
    const result = f.run();
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Created verified manifest commit/);

    const raw = readFileSync(f.capture, 'utf8').split('\0').filter(Boolean);
    const args = new Map();
    for (let i = 0; i < raw.length; i++) {
      if (raw[i] === '-f') {
        const [key, ...rest] = raw[++i].split('=');
        args.set(key, rest.join('='));
      }
    }
    assert.equal(args.get('repository'), 'MFSGA/Chimera');
    assert.equal(args.get('refName'), 'master');
    assert.equal(
      args.get('expectedHeadOid'),
      git(f.dir, ['rev-parse', 'HEAD']),
    );
    assert.equal(
      Buffer.from(args.get('contents'), 'base64').toString(),
      changed,
    );
    assert.match(args.get('query'), /createCommitOnBranch/);
    assert.match(args.get('query'), /expectedHeadOid/);
    assert.match(args.get('query'), /manifest\/version\.json/);
  } finally {
    f.cleanup();
  }
});

test('skips publishing when manifest is unchanged', () => {
  const f = fixture();
  try {
    const result = f.run();
    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /unchanged/);
    assert.equal(existsSync(f.capture), false);
  } finally {
    f.cleanup();
  }
});

test('rejects an invalid or missing GitHub signature', () => {
  const f = fixture();
  try {
    writeFileSync(f.manifest, '{"version":"updated"}\n');
    for (const mockResponse of [
      verifiedResponse.replace('"isValid":true', '"isValid":false'),
      JSON.stringify({ errors: [{ message: 'Missing branch permission' }] }),
    ]) {
      const result = f.run({ mockResponse });
      assert.notEqual(result.status, 0);
      assert.match(result.stderr, /not verified/);
    }
  } finally {
    f.cleanup();
  }
});

test('refuses the wrong checkout and propagates a rejected stale-head update', () => {
  const f = fixture();
  try {
    writeFileSync(f.manifest, '{"version":"updated"}\n');
    const wrongBranch = f.run({ target: 'dev' });
    assert.notEqual(wrongBranch.status, 0);
    assert.match(wrongBranch.stderr, /Checkout must be on dev/);
    assert.equal(existsSync(f.capture), false);

    const stale = f.run({ mockExitCode: '1' });
    assert.notEqual(stale.status, 0);
  } finally {
    f.cleanup();
  }
});
