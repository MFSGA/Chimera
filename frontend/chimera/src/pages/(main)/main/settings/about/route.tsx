import { createFileRoute } from '@tanstack/react-router';
import z from 'zod';
import { PageTitle } from '@/components/page-title';
import * as m from '@/paraglide/messages';
import ChimeraVersion from './_modules/chimera-version';

export enum Action {
  NEED_UPDATE = 'need-update',
}

export const Route = createFileRoute('/(main)/main/settings/about')({
  component: RouteComponent,
  staticData: { crumb: { title: () => m.settings_label_about() } },
  validateSearch: z.object({
    action: z.enum(Action).optional().nullable(),
  }),
});

function RouteComponent() {
  return (
    <>
      <PageTitle legacySlot="settings-title" />

      <div className="space-y-4 px-4 pb-4">
        <div className="grid gap-2 sm:grid-cols-2">
          <ChimeraVersion />
        </div>
      </div>
    </>
  );
}
