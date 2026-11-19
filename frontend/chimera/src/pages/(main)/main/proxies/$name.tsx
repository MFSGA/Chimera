import { createFileRoute, redirect } from '@tanstack/react-router';

/**
 * Chimera previously used /main/proxies/$name for group details.
 * Keep existing bookmarks and cross-window links valid while the primary UI
 * follows ref's /main/proxies/group/$name route structure.
 * This route renders no duplicate detail page and never mutates a Profile.
 */
export const Route = createFileRoute('/(main)/main/proxies/$name')({
  beforeLoad: ({ params, search }) => {
    throw redirect({
      to: '/main/proxies/group/$name',
      params: { name: params.name },
      search,
      replace: true,
    });
  },
});
