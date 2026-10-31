import { Button } from '@chimera/ui/button';
import { EmptyState } from '@chimera/ui/empty-state';
import { Link } from '@tanstack/react-router';
import FolderOffOutlineRounded from '~icons/material-symbols/folder-off-outline-rounded';
import * as m from '@/paraglide/messages';

export default function FolderNotFound() {
  return (
    <EmptyState
      className="flex-1"
      icon={FolderOffOutlineRounded}
      message={m.profile_folder_not_found()}
      data-slot="profiles-folder-not-found"
    >
      <Button variant="stroked" className="px-4" asChild>
        <Link to="/main/profiles/$type" params={{ type: 'profile' }}>
          {m.profile_nav_all_profiles()}
        </Link>
      </Button>
    </EmptyState>
  );
}
