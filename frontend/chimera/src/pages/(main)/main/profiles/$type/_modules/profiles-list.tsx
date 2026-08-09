import { useProfile } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { move } from '@dnd-kit/helpers';
import { DragDropProvider } from '@dnd-kit/react';
import { isEqual } from 'lodash-es';
import type { ComponentProps } from 'react';
import { CircularProgress } from '@/components/ui/progress';
import * as m from '@/paraglide/messages';
import { Route as IndexRoute } from '../index';
import SortableProfileItem from './sortable-profile-item';
import { categoryProfiles } from './utils';

const EmptyList = ({ children }: { children?: React.ReactNode }) => (
  <div
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
  const { query, sort } = useProfile();

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

  const categorized = categoryProfiles(query.data?.items ?? []);
  const filteredProfiles = categorized[type as keyof typeof categorized] ?? [];

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
            className={cn(
              'grid content-start gap-2',
              'md:grid-cols-2',
              'lg:grid-cols-3',
              'dxl:grid-cols-4',
              className,
            )}
            data-slot="profiles-navigate"
          >
            {filteredProfiles.map((profile, index) => (
              <SortableProfileItem
                key={profile.uid}
                item={profile}
                index={index}
                disabled={sort.isPending}
              />
            ))}
          </div>
        </DragDropProvider>

        <div className="flex-1" />
      </div>

      <div className="mb-4 flex h-16 items-center justify-center text-center text-sm text-gray-500">
        {m.profile_no_more_profiles()}
      </div>
    </>
  );
}
