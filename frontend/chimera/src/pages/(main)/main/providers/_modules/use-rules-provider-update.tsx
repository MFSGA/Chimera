/**
 * Rules Provider 更新 hook
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-rules-provider-update.tsx`
 *
 * 使用 Chimera 的 useBlockTask 封装单个 rule provider 的更新操作
 */

import type { ClashRulesProviderQueryItem } from '@chimera/interface';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const useRulesProviderUpdate = (data: ClashRulesProviderQueryItem) => {
  const blockTask = useBlockTask(
    `update-rules-provider-${data.name}`,
    async () => {
      try {
        await data.mutate();
      } catch (error) {
        console.error('Failed to update rules provider', error);
        await message(`Update provider failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
        });
      }
    },
  );

  return blockTask;
};
