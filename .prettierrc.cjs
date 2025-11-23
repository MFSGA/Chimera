const sortAndTailwindPlugins = [
  '@ianvs/prettier-plugin-sort-imports',
  'prettier-plugin-tailwindcss',
];

const tailwindOnlyPlugins = ['prettier-plugin-tailwindcss'];

const tomlPlugins = ['prettier-plugin-toml'];

/** @type {import("prettier").Config} */
module.exports = {
  endOfLine: 'lf',
  semi: true,
  singleQuote: true,
  bracketSpacing: true,
  tabWidth: 2,
  trailingComma: 'all',
  overrides: [
    {
      files: ['tsconfig.json', 'jsconfig.json'],
      options: {
        parser: 'jsonc',
      },
    },
    {
      files: ['*.{js,jsx,ts,tsx,cjs,mjs,cts,mts}'],
      options: {
        plugins: sortAndTailwindPlugins,
        importOrder: ['^@root/(.*)$', '^@/(.*)$', '^~/(.*)$', '^[./]'],
        importOrderParserPlugins: ['typescript', 'jsx', 'decorators-legacy'],
        importOrderTypeScriptVersion: '5.0.0',
      },
    },
    {
      files: ['*.{html,htm,mdx,astro,vue,svelte}'],
      options: {
        plugins: tailwindOnlyPlugins,
      },
    },
    {
      files: ['*.toml'],
      options: {
        plugins: tomlPlugins,
      },
    },
  ],
};
