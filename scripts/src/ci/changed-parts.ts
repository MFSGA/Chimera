import { fromFileUrl, globToRegExp } from '@std/path';

/** The parts of the repository CI checks independently. */
export type Part = 'frontend' | 'backend' | 'scripts';

export const PARTS: readonly Part[] = ['frontend', 'backend', 'scripts'];

export interface ChangeRule {
  /** Repository-relative globs; `*` stays within one path segment. */
  patterns: string[];
  /** The parts a matching file needs checked; empty needs only Prettier. */
  parts: Part[];
}

/**
 * Ordered rules: the first rule matching a changed file decides its parts.
 * A file no rule matches checks every part, so new paths are over-checked.
 */
export const CHANGE_RULES: readonly ChangeRule[] = [
  {
    patterns: [
      'docs/**',
      '*.md',
      'LICENSE',
      '.editorconfig',
      '.husky/**',
      '.serena/**',
      '.vscode/**',
    ],
    parts: [],
  },
  {
    // Root package metadata is consumed by frontend, backend builds and scripts.
    patterns: [
      'package.json',
      'pnpm-lock.yaml',
      'pnpm-workspace.yaml',
      '.npmrc',
    ],
    parts: ['frontend', 'backend', 'scripts'],
  },
  {
    patterns: [
      'frontend/**',
      'tauri-e2e/**',
      'tsconfig*.json',
      'vitest.config.*',
      'knip.config.*',
      '.oxlintrc*',
      '.prettier*',
      '.stylelint*',
    ],
    parts: ['frontend'],
  },
  {
    patterns: ['backend/**', '.cargo/**', 'rust-toolchain*', '.gitmodules'],
    parts: ['backend'],
  },
  {
    patterns: ['scripts/**', '.github/workflows/target-release-build.yaml'],
    parts: ['scripts'],
  },
];

const compiledRules = CHANGE_RULES.map((rule) => ({
  regexps: rule.patterns.map((pattern) => globToRegExp(pattern)),
  parts: rule.parts,
}));

export function partsOf(file: string): readonly Part[] {
  return (
    compiledRules.find((rule) =>
      rule.regexps.some((regexp) => regexp.test(file)),
    )?.parts ?? PARTS
  );
}

export function changedParts(files: Iterable<string>): Set<Part> {
  const parts = new Set<Part>();
  for (const file of files) {
    for (const part of partsOf(file)) parts.add(part);
  }
  return parts;
}

async function changedFiles(cwd: string, base: string) {
  const result = await new Deno.Command('git', {
    cwd,
    args: ['diff', '--name-only', '--no-renames', '-z', base, 'HEAD', '--'],
    stdout: 'piped',
    stderr: 'piped',
  }).output();
  if (result.code !== 0) {
    throw new Error(new TextDecoder().decode(result.stderr));
  }
  return new TextDecoder().decode(result.stdout).split('\0').filter(Boolean);
}

if (import.meta.main) {
  // Without a base (pushes pass an empty one), every part is checked.
  const base = Deno.args[0];
  const workspaceRoot = fromFileUrl(new URL('../../..', import.meta.url));
  const parts = base
    ? changedParts(await changedFiles(workspaceRoot, base))
    : new Set(PARTS);

  const lines = PARTS.map((part) => `${part}=${parts.has(part)}`);
  console.log(lines.join('\n'));

  const output = Deno.env.get('GITHUB_OUTPUT');
  if (output) {
    await Deno.writeTextFile(output, `${lines.join('\n')}\n`, {
      append: true,
    });
  }
}
