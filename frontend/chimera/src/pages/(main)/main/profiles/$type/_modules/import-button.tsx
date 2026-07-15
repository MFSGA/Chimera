import { cn } from '@chimera/ui';
import CloudDownloadRounded from '@mui/icons-material/CloudDownloadRounded';
import FileOpenRounded from '@mui/icons-material/FileOpenRounded';
import NoteAddRounded from '@mui/icons-material/NoteAddRounded';
import { Fab, Tooltip } from '@mui/material';
import { useEffect, useMemo, useState } from 'react';
import {
  AddProfileContext,
  ProfileDialog,
  type AddProfileContextValue,
} from '@/components/profiles/profile-dialog';
import * as m from '@/paraglide/messages';
import { Action, Route as IndexRoute } from '../index';

type ImportType = NonNullable<AddProfileContextValue['type']>;

const ImportOption = ({
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
  <Tooltip title={label} placement="left">
    <Fab
      className={cn(
        '!absolute !left-1/2 !size-10 !-translate-x-1/2 !scale-0 !opacity-0',
        '!pointer-events-none !transition-[bottom,opacity,transform] !duration-300',
        'group-data-[expanded=true]:!pointer-events-auto group-data-[expanded=true]:!scale-100 group-data-[expanded=true]:!opacity-100',
        className,
      )}
      size="small"
      color="default"
      aria-label={label}
      onClick={onClick}
    >
      {children}
    </Fab>
  </Tooltip>
);

export default function ImportButton() {
  const { action } = IndexRoute.useSearch();
  const navigate = IndexRoute.useNavigate();
  const [expanded, setExpanded] = useState(false);
  const [importType, setImportType] = useState<ImportType | null>(null);

  useEffect(() => {
    if (action === Action.ImportLocalProfile) {
      setExpanded(false);
      setImportType('local');
    }
  }, [action]);

  const contextValue = useMemo<AddProfileContextValue | null>(
    () =>
      importType
        ? {
            type: importType,
            name: null,
            desc: null,
            url: '',
          }
        : null,
    [importType],
  );

  const openImport = (type: ImportType) => {
    setExpanded(false);
    setImportType(type);
  };

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

  return (
    <AddProfileContext.Provider value={contextValue}>
      <div
        className="group fixed right-8 bottom-8 z-10 size-14"
        data-expanded={String(expanded)}
        data-slot="profile-import-button"
      >
        <ImportOption
          className="!bottom-28"
          label={m.profile_import_remote_title()}
          onClick={() => openImport('remote')}
        >
          <CloudDownloadRounded fontSize="small" />
        </ImportOption>

        <ImportOption
          className="!bottom-14"
          label={m.profile_import_local_title()}
          onClick={() => openImport('local')}
        >
          <FileOpenRounded fontSize="small" />
        </ImportOption>

        <Fab
          className="!size-14"
          color="primary"
          aria-label={m.profile_create_title()}
          onClick={() => setExpanded((value) => !value)}
        >
          <NoteAddRounded />
        </Fab>
      </div>

      {importType && <ProfileDialog open onClose={closeImport} />}
    </AddProfileContext.Provider>
  );
}
