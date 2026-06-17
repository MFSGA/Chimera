/**
 * 代理图标组件
 *
 * 迁移自 ref: `src/pages/(main)/main/rules/_modules/proxy-icon.tsx`
 *
 * 职责：
 * - 根据代理组名称显示对应的图标
 * - 如果有 icon URL 则使用 CacheImage 渲染
 * - 否则显示代理组名称的前两个大写字母作为 fallback
 *
 * 用于规则页面的侧栏代理过滤列表，以及连接页面的侧栏代理过滤列表。
 */

import { useClashProxies } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { useMemo } from 'react';

/**
 * 代理图标组件
 *
 * @param groupName - 代理组名称
 * @returns 代理图标（图片或文字 fallback）
 */
export default function ProxyIcon({ groupName }: { groupName: string }) {
  const {
    proxies: { data: proxies },
  } = useClashProxies();

  // 从代理数据中查找对应组的 icon
  const icon = useMemo(() => {
    const proxyInfo = proxies?.groups.find((p) => p.name === groupName);

    return proxyInfo?.icon;
  }, [groupName, proxies]);

  // 如果有 icon URL 则渲染图片，否则显示文字 fallback
  return icon ? (
    <img
      className="size-6 rounded-full"
      src={icon}
      alt={groupName}
      loading="lazy"
    />
  ) : (
    <div
      className={cn(
        'bg-surface text-secondary grid size-6 place-content-center rounded-full text-[10px]',
      )}
    >
      {groupName?.toLocaleUpperCase().slice(0, 2)}
    </div>
  );
}
