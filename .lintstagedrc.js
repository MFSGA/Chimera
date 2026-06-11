export default {
  'scripts/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix .',
    // () => 'tsc -p scripts/tsconfig.json --noEmit',
  ],
  '*.{js,cjs,.mjs,jsx}': (filenames) => {
    const configFiles = [
      '.oxlintrc.json',
      '.lintstagedrc.js',
      'commitlint.config.js',
    ];
    const filtered = filenames.filter(
      (file) => !configFiles.some((config) => file.endsWith(config)),
    );
    if (filtered.length === 0) return [];
    return ['prettier --write', 'oxlint --fix'];
  },
  'frontend/interface/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
    () => 'tsc -p frontend/interface/tsconfig.json --noEmit',
  ],
  'frontend/utils/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
    () => 'tsc -p frontend/utils/tsconfig.json --noEmit',
  ],
  'frontend/ui/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
    () => 'tsc -p frontend/ui/tsconfig.json --noEmit',
  ],
  'frontend/chimera/**/*.{ts,tsx}': [
    'prettier --write',
    'oxlint --fix',
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
  '*.{md,json,jsonc,json5,yaml,yml,toml}': (filenames) => {
    // exclude generated locale files
    const filtered = filenames.filter(
      (file) => !file.includes('frontend/chimera/src/paraglide/'),
    );
    if (filtered.length === 0) return [];
    return `prettier --write ${filtered.join(' ')}`;
  },
};
