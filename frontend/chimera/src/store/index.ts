import { FileRouteTypes } from "@/routeTree.gen";
import { atomWithStorage } from "jotai/utils";

export const memorizedRoutePathAtom = atomWithStorage<
    FileRouteTypes['fullPaths'] | null
>('memorizedRoutePathAtom', null)
