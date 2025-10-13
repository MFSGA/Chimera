export default {
  '*.{js,cjs,.mjs,jsx}': ['prettier --write', 'eslint --cache --fix'],
  'scripts/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p scripts/tsconfig.json --noEmit',
  ],
  // todo
  'frontend/ui/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p frontend/ui/tsconfig.json --noEmit',
  ],
  'frontend/chimera/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p frontend/chimera/tsconfig.json --noEmit',
  ],
  'backend/**/*.{rs,toml}': [
    () =>
      'cargo clippy --manifest-path=./backend/Cargo.toml --all-targets --all-features',
    () => 'cargo fmt --manifest-path ./backend/Cargo.toml --all',
    // () => 'cargo test --manifest-path=./backend/Cargo.toml',
    // () => "cargo fmt --manifest-path=./backend/Cargo.toml --all",
    // do not submit untracked files
    // () => 'git add -u',
  ],
  '*.{html,sass,scss,less}': ['prettier --write', 'stylelint --fix'],
  'package.json': ['prettier --write'],
  '*.{md,json,jsonc,json5,yaml,yml,toml}': ['prettier --write'],
};
