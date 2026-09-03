import { collectEnvs, openThat } from '@chimera/interface';
import { formatEnvInfos } from '@/utils';

const CHIMERA_REPOSITORY_URL = 'https://github.com/MFSGA/Chimera';
const CHIMERA_BUG_REPORT_URL =
  'https://github.com/MFSGA/Chimera/issues/new?assignees=&labels=T%3A+Bug%2CS%3A+Untriaged&projects=&template=bug_report.yaml&env_infos=';

export const openProjectRepository = async () => {
  await openThat(CHIMERA_REPOSITORY_URL);
};

export const openBugReport = async () => {
  let envs;

  try {
    envs = await collectEnvs();
  } catch {
    return false;
  }

  const formattedEnv = encodeURIComponent(
    formatEnvInfos(envs)
      .split('\n')
      .map((value) => `> ${value}`)
      .join('\n'),
  );

  await openThat(CHIMERA_BUG_REPORT_URL + formattedEnv);
  return true;
};
