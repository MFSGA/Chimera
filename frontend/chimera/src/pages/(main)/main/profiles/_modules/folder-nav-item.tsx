import type { ProfileFolder } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Button } from '@chimera/ui/button';
import { useSortable } from '@dnd-kit/react/sortable';
import { Link } from '@tanstack/react-router';
import FolderRounded from '~icons/material-symbols/folder-rounded';
import { FOLDER_ITEM_DRAG_SENSORS } from '@/utils/dnd-sensors';
import FolderActionsMenu from './folder-actions-menu';
import {
  FOLDER_DRAG_TYPE,
  PROFILE_DRAG_TYPE,
  sidebarFolderId,
} from './resolve-profile-drop';

/** Ref-compatible folder item, adapted to Chimera's retained sidebar links. */
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
      className={cn(
        'flex min-w-0 items-center gap-1 rounded-xl px-2',
        'hover:bg-surface-variant/30',
        isDragging && 'opacity-40',
        isDropTarget && 'bg-primary-container/50',
      )}
      data-slot="profiles-folder-nav-item"
      data-folder={folder.id}
      data-dragging={isDragging || undefined}
      data-drop-target={isDropTarget || undefined}
    >
      <Button
        asChild
        className="min-w-0 flex-1 justify-start px-2"
        variant="basic"
      >
        <Link
          ref={ref}
          role="link"
          to="/main/profiles/folder/$folderId"
          params={{ folderId: folder.id }}
          aria-label={`${folder.name}, ${folder.members.length}`}
        >
          <FolderRounded className="size-5 shrink-0" aria-hidden />
          <span className="min-w-0 flex-1 truncate text-left">
            {folder.name}
          </span>
          <span className="text-on-surface-variant text-xs">
            {folder.members.length}
          </span>
        </Link>
      </Button>
      <FolderActionsMenu folder={folder.id} />
    </div>
  );
}
