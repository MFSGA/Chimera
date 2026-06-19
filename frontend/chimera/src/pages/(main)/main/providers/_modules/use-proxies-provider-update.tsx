/**
 * Proxies Provider 更新 hook
 *
 * 迁移自 ref: `ref/frontend/nyanpasu/src/pages/(main)/main/providers/_modules/use-proxies-provider-update.tsx`
 *
 * 使用 Chimera 的 useBlockTask 封装单个 proxy provider 的更新操作
 */

import type { ClashProxiesProviderQueryItem } from '@chimera/interface';
import { useBlockTask } from '@/components/providers/block-task-provider';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

export const useProxiesProviderUpdate = (
  data: ClashProxiesProviderQueryItem,
) => {
  const blockTask = useBlockTask(
    `update-proxies-provider-${data.name}`,
    async () => {
      try {
        await data.mutate();
      } catch (error) {
        console.error('Failed to update proxies provider', error);
        await message(`Update provider failed: \n ${formatError(error)}`, {
          title: 'Error',
          kind: 'error',
        });
      }
    },
  );

  return blockTask;
};
