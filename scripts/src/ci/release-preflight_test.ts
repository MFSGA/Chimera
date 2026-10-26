import assert from 'node:assert/strict';
import { dirname, fromFileUrl, join } from '@std/path';

const workspaceRoot = dirname(
  dirname(dirname(dirname(fromFileUrl(import.meta.url)))),
);
const releaseWorkflow = join(
  workspaceRoot,
  '.github/workflows/target-release-build.yaml',
);
const validator = join(workspaceRoot, 'scripts/ci/validate-release-draft.sh');

interface RunOptions {
  mode: 'eventual' | 'published' | 'permission-error' | 'missing';
  tag?: string;
}

async function runValidator({ mode, tag = 'v1.2.3' }: RunOptions) {
  const tempDir = await Deno.makeTempDir({
    prefix: 'chimera-release-preflight-',
  });
  const binDir = join(tempDir, 'bin');
  const callCount = join(tempDir, 'gh-call-count');
  await Deno.mkdir(binDir);
  await Deno.writeTextFile(callCount, '0');
  await Deno.writeTextFile(
    join(binDir, 'gh'),
    `#!/usr/bin/env bash
set -euo pipefail
count=$(<"$GH_CALL_COUNT")
count=$((count + 1))
printf '%s' "$count" > "$GH_CALL_COUNT"
case "$GH_MODE:$count" in
  eventual:1|eventual:2|missing:*)
    echo 'release not found' >&2
    exit 1
    ;;
  eventual:*) echo true ;;
  published:*) echo false ;;
  permission-error:*)
    echo 'HTTP 403: Resource not accessible by integration' >&2
    exit 1
    ;;
esac
`,
    { mode: 0o755 },
  );
  await Deno.writeTextFile(
    join(binDir, 'sleep'),
    '#!/usr/bin/env bash\nexit 0\n',
    { mode: 0o755 },
  );

  try {
    const result = await new Deno.Command('bash', {
      args: [validator],
      cwd: workspaceRoot,
      env: {
        GH_CALL_COUNT: callCount,
        GH_MODE: mode,
        GITHUB_REPOSITORY: 'example/chimera',
        PATH: `${binDir}:${Deno.env.get('PATH') ?? ''}`,
        RELEASE_TAG: tag,
      },
      stdout: 'piped',
      stderr: 'piped',
    }).output();
    return {
      code: result.code,
      count: Number(await Deno.readTextFile(callCount)),
      stderr: new TextDecoder().decode(result.stderr),
      stdout: new TextDecoder().decode(result.stdout),
    };
  } finally {
    await Deno.remove(tempDir, { recursive: true });
  }
}

Deno.test('retries eventual 404s and accepts a visible draft', async () => {
  const result = await runValidator({ mode: 'eventual' });
  assert.equal(result.code, 0);
  assert.equal(result.count, 3);
  assert.match(result.stdout, /is a draft; continuing/);
});

Deno.test('rejects a release that is no longer a draft', async () => {
  const result = await runValidator({ mode: 'published' });
  assert.equal(result.code, 1);
  assert.equal(result.count, 1);
  assert.match(result.stderr, /must still be a draft/);
});

Deno.test('fails immediately on non-404 GitHub errors', async () => {
  const result = await runValidator({ mode: 'permission-error' });
  assert.equal(result.code, 1);
  assert.equal(result.count, 1);
  assert.match(result.stderr, /HTTP 403/);
});

Deno.test(
  'bounds retries when the release remains undiscoverable',
  async () => {
    const result = await runValidator({ mode: 'missing' });
    assert.equal(result.code, 1);
    assert.equal(result.count, 12);
    assert.match(result.stderr, /not discoverable after 12 attempts/);
  },
);

Deno.test('rejects an invalid tag without querying GitHub', async () => {
  const result = await runValidator({ mode: 'eventual', tag: 'latest' });
  assert.equal(result.code, 1);
  assert.equal(result.count, 0);
  assert.match(result.stderr, /Invalid release tag/);
});

Deno.test(
  'release preflight has permission to inspect draft releases',
  async () => {
    const workflow = await Deno.readTextFile(releaseWorkflow);
    const preflightJob = workflow.match(
      /^  release_preflight:\n([\s\S]*?)(?=^  [a-z0-9_]+:\n)/m,
    )?.[1];

    assert.ok(preflightJob, 'release_preflight job is present');
    assert.match(
      preflightJob,
      /^    permissions:\r?\n(?:      #[^\r\n]*\r?\n)*      contents: write\r?$/m,
    );
  },
);
