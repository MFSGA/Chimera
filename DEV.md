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

# problems
## `[vite] Internal server error: Failed to resolve import "@/store" from "src/pages/index.tsx?tsr-split=component". Does the file exist?`
set the `alias` in vite.config.ts