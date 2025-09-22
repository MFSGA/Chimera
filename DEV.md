# project deps
## ``

# frontend deps
## ``

# about start order
## dev
`pnpm dev` -> 
`pnpm run web:dev` ->
`pnpm --filter=chimera-ui dev`

## build
`pnpm build`

## lint
### eslint settings
    "eslint": "^9.35.0",
    "eslint-config-prettier": "^10.1.8",
    "eslint-plugin-prettier": "^5.5.4",
    "eslint-plugin-react": "^7.37.5",
    "eslint-plugin-react-hooks": "^5.2.0",
    "@eslint/js": "^9.35.0",
    "@typescript-eslint/eslint-plugin": "^8.43.0",
    "@typescript-eslint/parser": "^8.43.0"

## update process
### local
1. install the dependency

2. generate the key pair.

3. set the `tauri.conf.json`
including the `plugins.updater`
pay attention to the `version` field in tauri.conf.json

4. set the permission in `capabilities` directory
    "updater:default"

### remote( update server )
5. set the json for update info.
file name is static -- update.json

### test the settings

## front end routes
### use the `tanstack` router
"@tanstack/router-plugin": "1.114.29",
"@tanstack/react-router": "1.114.29"

pay attention to the `Component` -- `<Outlet />`
### use the dev tools for router
`"@tanstack/react-router-devtools": "1.131.35"`

### important files
`__root.tsx`: rendered root.
`_layout.tsx`: render layout.

# build workflow
Lint Success
Setup Environment
Install Dependency
Prepare Sidecar and Resources
Build Frontend

根据我对根路径文件的分析，这些配置文件主要分为以下几个类别：

## 项目管理和依赖配置

**package.json** - 这是项目的核心配置文件，定义了项目信息、依赖包、脚本命令等。它配置了这是一个名为"@nyanpasu/monorepo"的monorepo项目，包含了开发、构建、测试、代码检查等各种npm脚本。 [1](#0-0) 

**renovate.json** - 自动化依赖更新工具Renovate的配置文件，用于自动检测和更新项目依赖。它配置了不同类型包的更新策略，比如npm包采用固定版本，cargo包更新lockfile等。 [2](#0-1) 

## 代码质量和格式化工具

**.prettierrc.cjs** - Prettier代码格式化工具的配置文件，定义了代码格式化规则，如使用单引号、行末不加分号、使用LF换行符等。还配置了导入语句的排序规则和各种插件。 [3](#0-2) 

**eslint.config.js** - ESLint代码检查工具的配置文件，用于检查JavaScript/TypeScript代码质量。它配置了针对React、TypeScript等的代码规则，以及针对不同文件类型的检查规则。 [4](#0-3) 

**.stylelintrc.js** - Stylelint CSS/SCSS代码检查工具的配置文件，用于检查样式文件的代码质量和格式。它支持SCSS语法，并配置了针对Tailwind CSS的特殊规则。 [5](#0-4) 

**.lintstagedrc.js** - lint-staged工具的配置文件，用于在Git提交前自动运行代码检查和格式化。它配置了针对不同文件类型（JS/TS、Rust、CSS等）的预提交钩子。 [6](#0-5) 

**commitlint.config.js** - commitlint工具的配置文件，用于检查Git提交信息的格式，确保提交信息符合约定式提交规范。 [7](#0-6) 

## 开发工具配置

**knip.config.ts** - Knip工具的配置文件，用于检测项目中未使用的代码、依赖和配置。它定义了项目的入口点和要检查的文件范围。 [8](#0-7) 

**cliff.toml** - git-cliff工具的配置文件，用于自动生成changelog（变更日志）。它配置了如何解析Git提交信息并生成格式化的变更日志，支持按提交类型分组和GitHub集成。 [9](#0-8) 

## Notes

这些配置文件形成了一个完整的现代前端开发工具链：
- **代码质量保证**：通过ESLint、Stylelint、Prettier确保代码质量和一致性
- **自动化流程**：通过lint-staged、commitlint、renovate实现开发流程自动化
- **项目维护**：通过knip检测无用代码，通过cliff自动生成变更日志
- **monorepo管理**：package.json配置了复杂的脚本来管理多个子项目

这是一个使用Tauri构建的桌面应用项目，同时包含Rust后端和React前端，因此配置文件既涵盖了前端工具链，也包含了Rust项目的代码检查配置。

### Citations

**File:** package.json (L1-10)
```json
{
  "name": "@nyanpasu/monorepo",
  "version": "2.0.0",
  "repository": "https://github.com/libnyanpasu/clash-nyanpasu.git",
  "license": "GPL-3.0",
  "type": "module",
  "scripts": {
    "dev": "run-p -r web:devtools tauri:dev",
    "dev:diff": "run-p -r web:devtools tauri:diff",
    "build": "tauri build",
```

**File:** renovate.json (L1-20)
```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": [
    "config:recommended",
    "default:automergeMinor",
    "default:prConcurrentLimit10",
    "default:prHourlyLimitNone",
    "default:preserveSemverRanges",
    "default:rebaseStalePrs",
    "group:monorepos"
  ],
  "packageRules": [
    {
      "matchManagers": ["npm"],
      "rangeStrategy": "pin"
    },
    {
      "matchManagers": ["cargo"],
      "rangeStrategy": "update-lockfile",
      "platformAutomerge": false
    }
  ]
```

**File:** .prettierrc.cjs (L2-15)
```javascript
module.exports = {
  endOfLine: 'lf',
  semi: false,
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
```

**File:** eslint.config.js (L36-70)
```javascript
export default tseslint.config(
  includeIgnoreFile(gitignorePath),
  {
    ignores,
  },
  {
    files: ['**/*.{jsx,mjsx,tsx,mtsx}'],
    extends: [
      // @ts-expect-error fucking plugin why export flat config with nullable types?
      react.configs.flat.recommended,
    ],
    plugins: {
      // @ts-expect-error react hooks not compatible with eslint types
      'react-hooks': pluginReactHooks,
      'react-compiler': pluginReactCompiler,
    },
    settings: {
      react: {
        version: 'detect',
      },
    },
    rules: {
      'react-hooks/rules-of-hooks': 'error',
      'react-hooks/exhaustive-deps': 'warn',
      'react-compiler/react-compiler': 'warn',
    },
  },
  {
    files: ['**/*.{js,mjs,cjs,jsx,mjsx,ts,tsx,mtsx}'],
    extends: [
      ...neostandard({ ts: true, semi: true, noStyle: true }),
      eslintConfigPrettier,
      eslintPluginPrettierRecommended,
    ],
    rules: {
```

**File:** .stylelintrc.js (L3-20)
```javascript
export default {
  root: true,
  defaultSeverity: 'error',
  plugins: [
    'stylelint-scss',
    'stylelint-order',
    'stylelint-declaration-block-no-ignored-properties',
  ],
  extends: [
    'stylelint-config-standard',
    'stylelint-config-html/html', // the shareable html config for Stylelint.
    'stylelint-config-recess-order',
    // 'stylelint-config-prettier'
  ],
  rules: {
    'selector-pseudo-class-no-unknown': [
      true,
      { ignorePseudoClasses: ['global'] },
```

**File:** .lintstagedrc.js (L1-10)
```javascript
export default {
  '*.{js,cjs,.mjs,jsx}': ['prettier --write', 'eslint --cache --fix'],
  'scripts/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
    () => 'tsc -p scripts/tsconfig.json --noEmit',
  ],
  'frontend/interface/**/*.{ts,tsx}': [
    'prettier --write',
    'eslint --cache --fix',
```

**File:** commitlint.config.js (L1-1)
```javascript
export default { extends: ['@commitlint/config-conventional'] }
```

**File:** knip.config.ts (L3-10)
```typescript
export default {
  entry: [
    'frontend/nyanpasu/src/main.tsx',
    'frontend/nyanpasu/src/pages/**/*.tsx',
    'scripts/*.{js,ts}',
  ],
  project: ['frontend/**/*.{ts,js,jsx,tsx}', 'scripts/**/*.{js,ts}'],
} satisfies KnipConfig
```

**File:** cliff.toml (L4-20)
```text
[changelog]
# changelog header
header = """
# Changelog\n
All notable changes to this project will be documented in this file.\n
"""
# template for the changelog body
# https://keats.github.io/tera/docs/#introduction
body = """
{% set whitespace = " " %}
{% if version %}\
    ## [{{ version | trim_start_matches(pat="v") }}] - {{ timestamp | date(format="%Y-%m-%d") }}
{% else %}\
    ## [unreleased]
{% endif %}\
{% for group, commits in commits | filter(attribute="breaking", value=true) | group_by(attribute="group") %}
    ### {{ group | upper_first }}
```


# problems
## `[vite] Internal server error: Failed to resolve import "@/store" from "src/pages/index.tsx?tsr-split=component". Does the file exist?`
set the `alias` in vite.config.ts