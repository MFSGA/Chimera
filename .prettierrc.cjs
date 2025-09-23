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
  ],
  importOrder: ['^@root/(.*)$', '^@/(.*)$', '^~/(.*)$', '^[./]'],
  importOrderParserPlugins: ['typescript', 'jsx', 'decorators-legacy'],
  importOrderTypeScriptVersion: '5.0.0',
  plugins: [
    //'@ianvs/prettier-plugin-sort-imports',
    'prettier-plugin-tailwindcss',
    'prettier-plugin-toml',
  ],
};
