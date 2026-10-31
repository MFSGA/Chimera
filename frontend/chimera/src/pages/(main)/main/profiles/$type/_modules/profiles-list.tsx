import { useProfile } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { CircularProgress } from '@chimera/ui/progress';
import { move } from '@dnd-kit/helpers';
import { DragDropProvider } from '@dnd-kit/react';
import { isEqual } from 'lodash-es';
import { useRef, type ComponentProps } from 'react';
import * as m from '@/paraglide/messages';
import { WHOLE_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import { ProfileType } from '../../_modules/consts';
import ProfileSections from '../../_modules/profile-sections';
import {
  NoMoreProfiles,
  useListOverflows,
} from '../../_modules/profiles-list-footer';
import { useProgressiveCount } from '../../_modules/use-progressive-count';
import { categoryProfiles } from '../../_modules/utils';
import { Route as IndexRoute } from '../index';
import SortableProfileItem from './sortable-profile-item';

const EmptyList = ({ children }: { children?: React.ReactNode }) => (
  <div
    data-slot="profiles-empty-list"
    className={cn(
      'flex min-h-full flex-1 items-center justify-center text-center text-sm',
      'text-on-surface-variant dark:text-on-surface-variant-dark',
    )}
  >
    {children ?? m.profile_empty_list_message()}
  </div>
);

export default function ProfilesList({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { type } = IndexRoute.useParams();
  return type === ProfileType.Profile ? (
    <ProfileSections className={className} {...props} />
  ) : (
    <TransformList className={className} {...props} />
  );
}

/** Keep the existing full-order persisted sorting for Transform tabs. */
function TransformList({
  className,
  ...props
}: Omit<ComponentProps<'div'>, 'children'>) {
  const { type } = IndexRoute.useParams();
  const { query, sort } = useProfile();
  const profiles = query.data?.items ?? [];
  const globalTransforms = new Set(query.data?.global_transforms ?? []);
  const categorized = categoryProfiles(profiles);
  const filteredProfiles = categorized[type as keyof typeof categorized] ?? [];
  const gridRef = useRef<HTMLDivElement>(null);
  const overflows = useListOverflows(gridRef, filteredProfiles.length);
  const shown = useProgressiveCount(filteredProfiles.length);

  if (query.isLoading) {
    return (
      <div className="flex min-h-full flex-1 items-center justify-center">
        <CircularProgress className="size-8" indeterminate />
      </div>
    );
  }

  if (query.isError) {
    return <EmptyList>{String(query.error)}</EmptyList>;
  }

  if (filteredProfiles.length === 0) {
    return <EmptyList />;
  }

  return (
    <>
      <div
        className={cn('flex min-h-full flex-1 flex-col gap-4')}
        data-slot="profiles-list"
        {...props}
      >
        <DragDropProvider
          sensors={WHOLE_ITEM_DRAG_SENSORS}
          onDragEnd={(event) => {
            const filteredUids = filteredProfiles.map((profile) => profile.uid);
            const nextFilteredUids = move(filteredUids, event);

            if (isEqual(filteredUids, nextFilteredUids)) return;

            const filteredSet = new Set(filteredUids);
            let cursor = 0;
            const fullOrder = (query.data?.items ?? []).map((item) =>
              filteredSet.has(item.uid) ? nextFilteredUids[cursor++] : item.uid,
            );

            sort.mutate(fullOrder);
          }}
        >
          <div
            ref={gridRef}
            className={cn(
              'grid content-start gap-2',
              'md:grid-cols-2',
              'lg:grid-cols-3',
              'dxl:grid-cols-4',
              className,
            )}
            data-slot="profiles-navigate"
            data-rendered-count={shown}
            data-total-count={filteredProfiles.length}
          >
            {filteredProfiles.slice(0, shown).map((profile, index) => (
              <SortableProfileItem
                key={profile.uid}
                item={profile}
                profiles={profiles}
                isGlobal={globalTransforms.has(profile.uid)}
                index={index}
                disabled={sort.isPending || query.data?.current === profile.uid}
              />
            ))}
          </div>
        </DragDropProvider>

        <div className="flex-1" />
      </div>

      {overflows && <NoMoreProfiles />}
    </>
  );
}
