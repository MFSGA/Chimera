import { createFileRoute, redirect } from '@tanstack/react-router';

/** Folder navigation lives in the sidebar, not under a folder index page. */
export const Route = createFileRoute('/(main)/main/profiles/folder/')({
  beforeLoad: () => {
    throw redirect({
      to: '/main/profiles/$type',
      params: { type: 'profile' },
      replace: true,
    });
  },
});
