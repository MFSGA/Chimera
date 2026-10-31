import type { ProfileFolder } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Link, useNavigate } from '@tanstack/react-router';
import ArrowBackIosNewRounded from '~icons/material-symbols/arrow-back-ios-new-rounded';
import * as m from '@/paraglide/messages';
import FolderActionsMenu from '../../../_modules/folder-actions-menu';

/** Folder page heading and the ref rename/delete controls. */
export default function FolderHeader({ folder }: { folder: ProfileFolder }) {
  const navigate = useNavigate();
  const handleDeleted = () => {
    void navigate({ to: '/main/profiles/$type', params: { type: 'profile' } });
  };

  return (
    <header
      className="bg-mixed-background sticky top-0 z-50 flex items-center justify-between gap-3 p-4"
      data-slot="profiles-folder-header"
    >
      <div className="flex min-w-0 items-center gap-2">
        <Button
          asChild
          icon
          className="size-9"
          aria-label={m.profile_nav_all_profiles()}
        >
          <Link to="/main/profiles/$type" params={{ type: 'profile' }}>
            <ArrowBackIosNewRounded className="size-4" aria-hidden />
          </Link>
        </Button>
        <h1 className="truncate text-lg font-bold">{folder.name}</h1>
      </div>
      <FolderActionsMenu folder={folder.id} onDeleted={handleDeleted} />
    </header>
  );
}
