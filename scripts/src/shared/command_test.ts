import assert from 'node:assert/strict';
import { test } from 'node:test';
import { exitStatus, runCommand, spawnCommand } from './command.ts';

test('runCommand captures output and a non-zero exit without throwing', async () => {
  const result = await runCommand(process.execPath, {
    args: [
      '-e',
      "process.stdout.write('out'); process.stderr.write('err'); process.exitCode = 3",
    ],
  });
  assert.deepStrictEqual(result.success, false);
  assert.deepStrictEqual(result.code, 3);
  assert.deepStrictEqual(result.stdout.toString(), 'out');
  assert.deepStrictEqual(result.stderr.toString(), 'err');
});

test('runCommand merges env into the parent environment unless cleared', async () => {
  const print = [
    '-e',
    'process.stdout.write(JSON.stringify([process.env.CHIMERA_A, process.env.CHIMERA_INHERITED]))',
  ];
  const previous = process.env.CHIMERA_INHERITED;
  process.env.CHIMERA_INHERITED = 'parent';
  try {
    const merged = await runCommand(process.execPath, {
      args: print,
      env: { CHIMERA_A: 'a' },
    });
    assert.deepStrictEqual(JSON.parse(merged.stdout.toString()), [
      'a',
      'parent',
    ]);
    const cleared = await runCommand(process.execPath, {
      args: print,
      env: { CHIMERA_A: 'a' },
      clearEnv: true,
    });
    assert.deepStrictEqual(JSON.parse(cleared.stdout.toString()), ['a', null]);
  } finally {
    if (previous === undefined) delete process.env.CHIMERA_INHERITED;
    else process.env.CHIMERA_INHERITED = previous;
  }
});

test('runCommand rejects when the program does not exist', async () => {
  await assert.rejects(() => runCommand('chimera-missing-program'), {
    code: 'ENOENT',
  });
});

test('spawnCommand feeds piped stdin and reports the exit status', async () => {
  const child = spawnCommand(process.execPath, {
    args: [
      '-e',
      "let data = ''; process.stdin.on('data', c => data += c); process.stdin.on('end', () => { process.exitCode = data === 'ping\\n' ? 0 : 1; });",
    ],
    stdin: 'piped',
    stderr: 'null',
  });
  child.stdin!.end('ping\n');
  assert.deepStrictEqual((await exitStatus(child)).success, true);
});

test('runCommand writes input to stdin and captures the output', async () => {
  const result = await runCommand(process.execPath, {
    args: ['-e', 'process.stdin.pipe(process.stdout)'],
    input: 'ping\n',
  });
  assert.deepStrictEqual(result.success, true);
  assert.deepStrictEqual(result.stdout.toString('utf8'), 'ping\n');
});

test(
  'exitStatus maps a signal exit to 128 plus the signal number',
  { skip: process.platform === 'win32' },
  async () => {
    const child = spawnCommand(process.execPath, {
      args: ['-e', "process.kill(process.pid, 'SIGTERM')"],
    });
    assert.deepStrictEqual(await exitStatus(child), {
      success: false,
      code: 143,
    });
  },
);
