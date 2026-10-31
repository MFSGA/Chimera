import type { FolderId } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@chimera/ui/dropdown-menu';
import ArrowDownwardRounded from '~icons/material-symbols/arrow-downward-rounded';
import ArrowUpwardRounded from '~icons/material-symbols/arrow-upward-rounded';
import DeleteForeverOutlineRounded from '~icons/material-symbols/delete-forever-outline-rounded';
import EditSquareOutlineRounded from '~icons/material-symbols/edit-square-outline-rounded';
import MoreVertRounded from '~icons/material-symbols/more-vert-rounded';
import * as m from '@/paraglide/messages';
import { useProfileLayout } from './use-profile-layout';

/** The overflow menu of a folder: rename it, or delete it. */
export default function FolderActionsMenu({
  folder,
  onDeleted,
}: {
  folder: FolderId;
  /** Called once the folder is deleted, not when the user declines. */
  onDeleted?: () => void;
}) {
  const { layout, openRenameFolder, deleteFolder, reorderFolders } =
    useProfileLayout();
  const folderIds = (layout?.folders ?? []).map((item) => item.id);
  const index = folderIds.indexOf(folder);

  const moveBy = (offset: -1 | 1) => {
    const next = [...folderIds];
    [next[index], next[index + offset]] = [next[index + offset], next[index]];
    void reorderFolders(next);
  };

  const handleRename = () => openRenameFolder(folder);

  const handleDelete = async () => {
    if (await deleteFolder(folder)) {
      onDeleted?.();
    }
  };

  return (
    <DropdownMenu>
      <DropdownMenuTrigger asChild>
        <Button
          icon
          className="size-9 shrink-0"
          aria-label={m.profile_detail_more_actions()}
          data-slot="profiles-folder-actions"
        >
          <MoreVertRounded className="size-5" aria-hidden />
        </Button>
      </DropdownMenuTrigger>

      <DropdownMenuContent align="end">
        <DropdownMenuItem onSelect={handleRename}>
          <EditSquareOutlineRounded className="size-4" aria-hidden />
          <span>{m.profile_folder_rename_title()}</span>
        </DropdownMenuItem>

        <DropdownMenuItem
          disabled={index <= 0}
          onSelect={() => moveBy(-1)}
          data-slot="profiles-folder-move-up"
        >
          <ArrowUpwardRounded className="size-4" aria-hidden />
          <span>{m.profile_folder_move_up()}</span>
        </DropdownMenuItem>

        <DropdownMenuItem
          disabled={index < 0 || index >= folderIds.length - 1}
          onSelect={() => moveBy(1)}
          data-slot="profiles-folder-move-down"
        >
          <ArrowDownwardRounded className="size-4" aria-hidden />
          <span>{m.profile_folder_move_down()}</span>
        </DropdownMenuItem>

        <DropdownMenuItem onSelect={handleDelete}>
          <DeleteForeverOutlineRounded className="size-4" aria-hidden />
          <span>{m.profile_folder_delete_title()}</span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
