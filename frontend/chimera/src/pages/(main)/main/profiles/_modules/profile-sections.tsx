import {
  isConfigItem,
  useProfile,
  type ProfileQueryResultItem,
} from '@chimera/interface';
import { cn } from '@chimera/ui';
import { CircularProgress } from '@chimera/ui/progress';
import { useRef, type ComponentProps } from 'react';
import * as m from '@/paraglide/messages';
import { useFolderCollapsed } from '../../_modules/use-folder-collapsed';
import SortableProfileItem from '../$type/_modules/sortable-profile-item';
import ProfileSection from './profile-section';
import { NoMoreProfiles, useListOverflows } from './profiles-list-footer';
import { sectionId } from './resolve-profile-drop';
import { useProfileLayout } from './use-profile-layout';
import { useProfilesDnd } from './use-profiles-dnd';
import { useProgressiveCount } from './use-progressive-count';

const COLLAPSED_KV_KEY = 'profiles-folder-collapsed';

/** Configs in their folders, followed by ungrouped cards; the active card stays above the sections. */
export default function ProfileSections({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { query } = useProfile();
  const profiles = query.data;
  const { layout } = useProfileLayout();
  const { items } = useProfilesDnd();
  const { isCollapsed, toggle } = useFolderCollapsed(COLLAPSED_KV_KEY);

  const configs = new Map<string, ProfileQueryResultItem>(
    (profiles?.items ?? [])
      .filter(isConfigItem)
      .map((item) => [item.uid, item]),
  );
  const globalTransforms = new Set(profiles?.global_transforms ?? []);
  const active = profiles?.current ? configs.get(profiles.current) : undefined;

  const listRef = useRef<HTMLDivElement>(null);
  const overflows = useListOverflows(listRef, configs.size);

  // Keep ref's six-card progressive rendering budget across the active card
  // and every section; do not render the same active card twice.
  const shown = useProgressiveCount(configs.size);
  let budget = shown;
  const take = <T,>(list: T[]) => {
    const taken = list.slice(0, budget);
    budget -= taken.length;
    return taken;
  };
  const shownActive = active && take([active])[0];

  if (query.isLoading) {
    return (
      <div className="flex min-h-full flex-1 items-center justify-center">
        <CircularProgress className="size-8" indeterminate />
      </div>
    );
  }
  if (query.isError) {
    return (
      <div className="flex min-h-full flex-1 items-center justify-center text-sm">
        {String(query.error)}
      </div>
    );
  }

  const folders = layout?.folders ?? [];
  // Keep empty folders visible and manageable; even when no configs remain.
  if (configs.size === 0 && folders.length === 0) {
    return (
      <div
        className="flex min-h-full flex-1 items-center justify-center text-sm"
        data-slot="profiles-empty-list"
      >
        {m.profile_empty_list_message()}
      </div>
    );
  }

  const renderSection = (
    folder: (typeof folders)[number] | null,
    title: string | undefined,
  ) => {
    const group = sectionId(folder?.id ?? null);
    const key = folder?.id ?? 'ungrouped';
    const sectionProfiles = (items[group] ?? []).flatMap(
      (uid) => configs.get(uid) ?? [],
    );

    if (!folder && folders.length === 0 && sectionProfiles.length === 0) {
      return null;
    }

    return (
      <ProfileSection
        key={key}
        folder={folder?.id ?? null}
        empty={sectionProfiles.length === 0}
        heading={
          title === undefined
            ? undefined
            : {
                title,
                count: sectionProfiles.length,
                collapsed: isCollapsed(key),
                handleToggle: () => toggle(key),
              }
        }
      >
        {take(sectionProfiles).map((profile, index) => (
          <SortableProfileItem
            key={profile.uid}
            item={profile}
            profiles={profiles?.items ?? []}
            group={group}
            index={index}
            disabled={query.isFetching || profiles?.current === profile.uid}
            isGlobal={globalTransforms.has(profile.uid)}
          />
        ))}
      </ProfileSection>
    );
  };

  return (
    <>
      <div
        className="flex min-h-full flex-1 flex-col gap-4"
        data-slot="profiles-list"
        {...props}
      >
        <div
          ref={listRef}
          className={cn(
            'flex flex-col',
            folders.length > 0 ? 'gap-6' : 'gap-3',
            className,
          )}
          data-slot="profiles-navigate"
          data-rendered-count={shown}
          data-total-count={configs.size}
        >
          {shownActive && (
            <div
              className="dxl:grid-cols-4 grid gap-2 md:grid-cols-2 lg:grid-cols-3"
              data-slot="profiles-active"
            >
              <SortableProfileItem
                item={shownActive}
                profiles={profiles?.items ?? []}
                index={0}
                disabled
                isGlobal={globalTransforms.has(shownActive.uid)}
              />
            </div>
          )}

          {folders.map((folder) => renderSection(folder, folder.name))}
          {renderSection(
            null,
            folders.length > 0
              ? m.profile_folder_section_ungrouped()
              : undefined,
          )}
        </div>
        <div className="flex-1" />
      </div>
      {overflows && <NoMoreProfiles />}
    </>
  );
}
