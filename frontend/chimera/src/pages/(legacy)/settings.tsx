import { commands, openThat } from '@chimera/interface';
import { BasePage } from '@chimera/ui';
import { Feedback, GitHub } from '@mui/icons-material';
import { IconButton } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { lazy } from 'react';
import { useTranslation } from 'react-i18next';
import { formatEnvInfos } from '@/utils';

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

  const FeedbackIcon = () => {
    const toFeedback = useLockFn(async () => {
      const envs = await commands.collectEnvs();

      if (envs.status !== 'ok') {
        return;
      }

      const formattedEnv = encodeURIComponent(
        formatEnvInfos(envs.data)
          .split('\n')
          .map((v) => `> ${v}`)
          .join('\n'),
      );

      return openThat(
        'https://github.com/MFSGA/Chimera/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml&env_infos=' +
          formattedEnv,
      );
    });

    return (
      <IconButton color="inherit" title={t('Feedback')} onClick={toFeedback}>
        <Feedback fontSize="inherit" />
      </IconButton>
    );
  };

  return (
    <BasePage
      title={t('Settings')}
      header={
        <div className="flex gap-1">
          <FeedbackIcon />
          <GithubIcon />
        </div>
      }
    >
      <Component />
    </BasePage>
  );
}
