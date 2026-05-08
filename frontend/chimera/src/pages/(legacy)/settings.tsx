import { openThat } from '@chimera/interface';
import { BasePage } from '@chimera/ui';
import { GitHub } from '@mui/icons-material';
import { IconButton } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { lazy } from 'react';
import { useTranslation } from 'react-i18next';

export const Route = createFileRoute('/(legacy)/settings')({
  component: SettingPage,
});

function SettingPage() {
  const { t } = useTranslation();
  // vital
  const Component = lazy(() => import('@/components/setting/setting-page'));

  const GithubIcon = () => {
    const toGithubRepo = useLockFn(() => {
      return openThat('https://github.com/MFSGA/Chimera');
    });

    return (
      <IconButton color="inherit" title="@MFSGA/Chimera" onClick={toGithubRepo}>
        <GitHub fontSize="inherit" />
      </IconButton>
    );
  };

  return (
    <BasePage
      title={t('Settings')}
      header={
        <div className="flex gap-1">
          <GithubIcon />
        </div>
      }
    >
      <Component />
    </BasePage>
  );
}
