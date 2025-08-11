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

# problems
## `[vite] Internal server error: Failed to resolve import "@/store" from "src/pages/index.tsx?tsr-split=component". Does the file exist?`
set the `alias` in vite.config.ts