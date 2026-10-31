import {
  isConfigItem,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { CircularProgress } from '@chimera/ui/progress';
import { createFileRoute } from '@tanstack/react-router';
import { useRef } from 'react';
import ImportButton from '../../_modules/import-button';
import ProfileQuickImport from '../../_modules/profile-quick-import';
import ProfileSection from '../../_modules/profile-section';
import {
  NoMoreProfiles,
  useListOverflows,
} from '../../_modules/profiles-list-footer';
import { sectionId } from '../../_modules/resolve-profile-drop';
import { useProfileLayout } from '../../_modules/use-profile-layout';
import { useProfilesDnd } from '../../_modules/use-profiles-dnd';
import { useProgressiveCount } from '../../_modules/use-progressive-count';
import SortableProfileItem from '../../$type/_modules/sortable-profile-item';
import FolderEmptyState from './_modules/folder-empty-state';
import FolderHeader from './_modules/folder-header';
import FolderNotFound from './_modules/folder-not-found';

export const Route = createFileRoute('/(main)/main/profiles/folder/$folderId/')(
  {
    component: RouteComponent,
  },
);

function RouteComponent() {
  const { folderId } = Route.useParams();
  const { layout } = useProfileLayout();
  const { query } = useProfile();
  const profiles = query.data;
  const { items } = useProfilesDnd();
  const listRef = useRef<HTMLDivElement>(null);
  const folder = layout?.folders.find((item) => item.id === folderId);
  const members = new Map<string, ProfileQueryResultItem>(
    (profiles?.items ?? [])
      .filter(isConfigItem)
      .map((item) => [item.uid, item]),
  );
  const active = folder?.members.includes(profiles?.current ?? '')
    ? members.get(profiles?.current ?? '')
    : undefined;
  const sectionProfiles = (items[sectionId(folderId)] ?? []).flatMap(
    (uid) => members.get(uid) ?? [],
  );
  const globalTransforms = new Set(profiles?.global_transforms ?? []);
  const listSize = sectionProfiles.length + (active ? 1 : 0);
  const overflows = useListOverflows(listRef, listSize);
  const shown = useProgressiveCount(listSize);
  const shownProfiles = sectionProfiles.slice(
    0,
    Math.max(0, shown - (active ? 1 : 0)),
  );

  if (!layout || query.isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <CircularProgress className="size-8" indeterminate />
      </div>
    );
  }
  if (query.isError) {
    return <div className="p-4 text-sm">{String(query.error)}</div>;
  }
  if (!folder) return <FolderNotFound />;

  return (
    <>
      <FolderHeader folder={folder} />
      <div className="px-4 pb-4">
        <ProfileQuickImport folder={folderId} />
      </div>
      {folder.members.length === 0 ? (
        <FolderEmptyState />
      ) : (
        <div
          ref={listRef}
          className="flex min-h-0 flex-1 flex-col gap-3 p-4 pt-0"
          data-slot="profiles-list"
          data-total-count={listSize}
          data-rendered-count={shown}
        >
          {active && shown > 0 && (
            <div
              className="dxl:grid-cols-4 grid gap-2 md:grid-cols-2 lg:grid-cols-3"
              data-slot="profiles-active"
            >
              <SortableProfileItem
                item={active}
                profiles={profiles?.items ?? []}
                index={0}
                disabled
                isGlobal={globalTransforms.has(active.uid)}
              />
            </div>
          )}
          {sectionProfiles.length > 0 && (
            <ProfileSection folder={folderId} empty={false}>
              {shownProfiles.map((profile, index) => (
                <SortableProfileItem
                  key={profile.uid}
                  item={profile}
                  profiles={profiles?.items ?? []}
                  group={sectionId(folderId)}
                  index={index}
                  disabled={
                    query.isFetching || profiles?.current === profile.uid
                  }
                  isGlobal={globalTransforms.has(profile.uid)}
                />
              ))}
            </ProfileSection>
          )}
        </div>
      )}
      {overflows && <NoMoreProfiles />}
      <ImportButton folder={folderId} />
    </>
  );
}
