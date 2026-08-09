import { createFileRoute } from '@tanstack/react-router';
import ImportButton from './_modules/import-button';
import ProfilesHeader from './_modules/profiles-header';
import ProfilesList from './_modules/profiles-list';

export enum Action {
  ImportLocalProfile = 'ImportLocalProfile',
}

type ProfileTypeSearch = {
  action?: Action;
};

export const Route = createFileRoute('/(main)/main/profiles/$type/')({
  validateSearch: (search): ProfileTypeSearch => ({
    action:
      search.action === Action.ImportLocalProfile
        ? Action.ImportLocalProfile
        : undefined,
  }),
  component: RouteComponent,
});

function RouteComponent() {
  return (
    <>
      <ProfilesHeader />
      <ProfilesList className="min-h-0 flex-1 p-4 pt-0" />
      <ImportButton />
    </>
  );
}
