import { cn } from '@chimera/ui';
import CloudDownloadRounded from '~icons/material-symbols/cloud-download-rounded';
import FileOpenRounded from '~icons/material-symbols/file-open-rounded';
import NoteStackAddRounded from '~icons/material-symbols/note-stack-add-rounded';
import { useEffect, useMemo, useState } from 'react';
import {
  AddProfileContext,
  ProfileDialog,
  type AddProfileContextValue,
} from '@/components/profiles/profile-dialog';
import { Button } from '@/components/ui/button';
import { useScrollArea } from '@/components/ui/scroll-area';
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from '@/components/ui/tooltip';
import * as m from '@/paraglide/messages';
import { ProfileType } from '../../_modules/consts';
import { Action, Route as IndexRoute } from '../index';

type ImportType = NonNullable<AddProfileContextValue['type']>;

const SelectButton = ({
  label,
  className,
  onClick,
  children,
}: {
  label: string;
  className?: string;
  onClick: () => void;
  children: React.ReactNode;
}) => (
  <Tooltip>
    <TooltipTrigger asChild>
      <Button
        variant="fab"
        icon
        aria-label={label}
        className={cn(
          'bg-primary-container dark:bg-surface-variant/30 flex size-10 items-center justify-center',
          className,
        )}
        onClick={onClick}
      >
        {children}
      </Button>
    </TooltipTrigger>
    <TooltipContent side="left">
      <span>{label}</span>
    </TooltipContent>
  </Tooltip>
);

export default function ImportButton() {
  const { type } = IndexRoute.useParams();
  const { action } = IndexRoute.useSearch();
  const navigate = IndexRoute.useNavigate();
  const { isScrolling } = useScrollArea();
  const [expanded, setExpanded] = useState(false);
  const [importType, setImportType] = useState<ImportType | null>(null);

  useEffect(() => {
    if (action === Action.ImportLocalProfile) {
      setExpanded(false);
      setImportType('local');
    }
  }, [action]);

  useEffect(() => {
    if (isScrolling && expanded) {
      setExpanded(false);
    }
  }, [expanded, isScrolling]);

  const contextValue = useMemo<AddProfileContextValue | null>(
    () =>
      importType ? { type: importType, name: null, desc: null, url: '' } : null,
    [importType],
  );

  const closeImport = () => {
    setImportType(null);
    if (action === Action.ImportLocalProfile) {
      void navigate({
        replace: true,
        search: (previous: { action?: Action }) => ({
          ...previous,
          action: undefined,
        }),
      } as never);
    }
  };

  if (type !== ProfileType.Profile) return null;

  return (
    <AddProfileContext.Provider value={contextValue}>
      <div
        className={cn(
          'absolute right-4 z-20 ml-auto w-fit',
          'top-[calc(100vh-40px-64px-72px)] sm:top-[calc(100vh-40px-48px-72px)]',
        )}
        data-slot="profile-import-button"
      >
        <div className="relative">
          <Button
            className="z-10"
            variant="fab"
            icon
            aria-label={m.profile_create_title()}
            onClick={() => setExpanded((value) => !value)}
          >
            <NoteStackAddRounded className="size-6" />
          </Button>

          <div
            className={cn(
              'absolute top-0 flex w-full flex-col items-center gap-4',
              'scale-0 opacity-0 transition-[top,opacity,scale] duration-300 ease-in-out',
              expanded && '-top-28 scale-100 opacity-100',
            )}
          >
            <SelectButton
              label={m.profile_import_remote_title()}
              onClick={() => {
                setExpanded(false);
                setImportType('remote');
              }}
            >
              <CloudDownloadRounded className="size-5" />
            </SelectButton>
            <SelectButton
              label={m.profile_import_local_title()}
              onClick={() => {
                setExpanded(false);
                setImportType('local');
              }}
            >
              <FileOpenRounded className="size-5" />
            </SelectButton>
          </div>
        </div>
      </div>

      {importType && <ProfileDialog open onClose={closeImport} />}
    </AddProfileContext.Provider>
  );
}
