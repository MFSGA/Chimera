import { atomWithStorage } from 'jotai/utils';
import { FileRouteTypes } from '@/routeTree.gen';

export const memorizedRoutePathAtom = atomWithStorage<
  FileRouteTypes['fullPaths'] | null
>('memorizedRoutePathAtom', null);
