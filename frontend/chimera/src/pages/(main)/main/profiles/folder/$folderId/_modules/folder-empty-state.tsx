import { EmptyState } from '@chimera/ui/empty-state';
import FolderOpenOutlineRounded from '~icons/material-symbols/folder-open-outline-rounded';
import * as m from '@/paraglide/messages';

/** Ref folder route empty state. A sidebar folder still receives dropped cards. */
export default function FolderEmptyState() {
  return (
    <EmptyState
      className="flex-1"
      icon={FolderOpenOutlineRounded}
      message={m.profile_folder_empty_title()}
      description={m.profile_folder_empty_description()}
      data-slot="profiles-folder-empty-state"
    />
  );
}
