const frontendTasks = (typecheckScript) => [
  'oxlint --fix',
  'prettier --write',
  () => `pnpm ${typecheckScript}`,
];

export default {
  // Scripts are currently a mixed Node/Deno tree and are excluded from oxlint.
  // Keep commit-time handling deterministic: format staged script files only.
  'scripts/**/*.{js,cjs,mjs,jsx,ts,tsx}': ['prettier --write'],

  'frontend/interface/**/*.{js,cjs,mjs,jsx,ts,tsx}': frontendTasks(
    'typecheck:interface',
  ),
  'frontend/utils/**/*.{js,cjs,mjs,jsx,ts,tsx}':
    frontendTasks('typecheck:utils'),
  'frontend/ui/**/*.{js,cjs,mjs,jsx,ts,tsx}': frontendTasks('typecheck:ui'),
  'frontend/chimera/**/*.{js,cjs,mjs,jsx,ts,tsx}':
    frontendTasks('typecheck:chimera'),
  'tauri-e2e/**/*.{js,cjs,mjs,jsx,ts,tsx}': frontendTasks('typecheck:e2e'),

  // Rust formatting is a check here instead of `cargo fmt --all`: a commit hook
  // must not rewrite unrelated or unstaged Rust files in the workspace.
  'backend/**/*.{rs,toml}': [
    () => 'pnpm lint:rustfmt',
    () => 'pnpm lint:clippy',
  ],

  '.lintstagedrc.js': ['prettier --write'],
  '.prettierrc.cjs': ['prettier --write'],
  '.stylelintrc.js': ['prettier --write'],
  'commitlint.config.js': ['prettier --write'],
  '*.{html,sass,scss,less}': ['stylelint --fix', 'prettier --write'],
  'cliff.toml': ['prettier --write'],
  '*.{md,json,jsonc,json5,yaml,yml}': ['prettier --write'],
};
