import { BasePage } from '@chimera/ui';
import { Feedback, GitHub } from '@mui/icons-material';
import { IconButton } from '@mui/material';
import { createFileRoute } from '@tanstack/react-router';
import { useLockFn } from 'ahooks';
import { lazy, Suspense } from 'react';
import {
  openBugReport,
  openProjectRepository,
} from '@/features/support/actions';
import * as m from '@/paraglide/messages';

const SettingPageComponent = lazy(
  () => import('@/components/setting/setting-page'),
);

export const Route = createFileRoute('/(legacy)/settings')({
  component: SettingPage,
});

function SettingPage() {
  const GithubIcon = () => {
    const toGithubRepo = useLockFn(openProjectRepository);

    return (
      <IconButton color="inherit" title="@MFSGA/Chimera" onClick={toGithubRepo}>
        <GitHub fontSize="inherit" />
      </IconButton>
    );
  };

  const FeedbackIcon = () => {
    const toFeedback = useLockFn(openBugReport);

    return (
      <IconButton color="inherit" title={'Feedback'} onClick={toFeedback}>
        <Feedback fontSize="inherit" />
      </IconButton>
    );
  };

  return (
    <BasePage
      title={m.navbar_label_settings()}
      header={
        <div className="flex gap-1">
          <FeedbackIcon />
          <GithubIcon />
        </div>
      }
    >
      <Suspense fallback={null}>
        <SettingPageComponent />
      </Suspense>
    </BasePage>
  );
}
