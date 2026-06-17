/**
 * Proxies 模块共享 Hooks
 *
 * 迁移自 ref: `src/pages/(main)/main/proxies/_modules/hooks.ts`
 *
 * 职责：
 * - useCurrentGroupConnection：查找经过当前代理组（链）的连接
 *   用于在 GroupHeader 中显示当前组的实时流量（下载/上传速度）
 */

import { useMemo } from 'react';
import {
  useClashConnections,
  type ClashProxiesQueryGroupItem,
} from '@chimera/interface';

/**
 * 查找经过当前代理组的活跃连接
 *
 * 遍历所有连接快照的最后一个（最新），
 * 找到 chains 中经过当前组名称的连接。
 *
 * @param currentGroup - 当前选中的代理组
 * @returns 经过当前组的连接对象，或 undefined
 */
export function useCurrentGroupConnection(
  currentGroup?: ClashProxiesQueryGroupItem,
) {
  const {
    query: { data: clashConnections },
  } = useClashConnections();

  return useMemo(() => {
    if (!currentGroup?.name) {
      return undefined;
    }

    return clashConnections
      ?.at(-1)
      ?.connections?.find((connection) =>
        connection.chains.includes(currentGroup?.name),
      );
  }, [clashConnections, currentGroup?.name]);
}
