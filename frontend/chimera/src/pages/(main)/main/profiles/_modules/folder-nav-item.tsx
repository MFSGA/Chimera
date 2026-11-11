import type { ProfileFolder } from '@chimera/interface';
import { cn } from '@chimera/utils';
import { useSortable } from '@dnd-kit/react/sortable';
import FolderRounded from '~icons/material-symbols/folder-rounded';
import * as m from '@/paraglide/messages';
import { FOLDER_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import FolderActionsMenu from './folder-actions-menu';
import ProfilesNavigationItem from './profiles-navigation-item';
import {
  FOLDER_DRAG_TYPE,
  PROFILE_DRAG_TYPE,
  sidebarFolderId,
} from './resolve-profile-drop';

/**
 * Ref NavigationList folder link with the Chimera-only visible action menu.
 * Keep the draggable target on the link, not the surrounding menu controls.
 */
export default function FolderNavItem({
  folder,
  index,
}: {
  folder: ProfileFolder;
  index: number;
}) {
  const { ref, isDragging, isDropTarget } = useSortable({
    id: sidebarFolderId(folder.id),
    index,
    group: 'folders',
    type: FOLDER_DRAG_TYPE,
    accept: [FOLDER_DRAG_TYPE, PROFILE_DRAG_TYPE],
    sensors: FOLDER_ITEM_DRAG_SENSORS,
  });

  return (
    <div
      className={cn('relative min-w-0', isDragging && 'opacity-40')}
      data-folder={folder.id}
      data-dragging={isDragging || undefined}
      data-drop-target={isDropTarget || undefined}
    >
      <ProfilesNavigationItem
        ref={ref}
        role="link"
        to="/main/profiles/folder/$folderId"
        params={{ folderId: folder.id }}
        color="neutral"
        icon={<FolderRounded />}
        label={folder.name}
        description={m.profile_nav_count_profiles({
          count: folder.members.length,
        })}
        aria-label={`${folder.name}, ${folder.members.length}`}
        className="pr-14"
        data-slot="profiles-folder-nav-item"
        data-dragging={isDragging || undefined}
        data-drop-target={isDropTarget || undefined}
      />
      <div className="absolute top-1/2 right-2 z-10 -translate-y-1/2">
        <FolderActionsMenu folder={folder.id} />
      </div>
    </div>
  );
}
