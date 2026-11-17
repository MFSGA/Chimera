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

import { useNativeProxies, useProxyGroupIcons } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { CacheImage } from '@/components/ui/image';

/**
 * 代理图标组件
 *
 * @param groupName - 代理组名称
 * @returns 代理图标（图片或文字 fallback）
 */
export default function ProxyIcon({ groupName }: { groupName: string }) {
  const { data: proxies } = useNativeProxies();

  // The ordinary native graph contains only a presence flag. The icon URL
  // comes from its separate query, matching the Proxies sidebar contract.
  const group = proxies?.groups.find((item) => item.name === groupName);
  const icons = useProxyGroupIcons();
  const icon = group?.iconConfigured ? icons[groupName] : undefined;

  return icon ? (
    <CacheImage
      className="size-6"
      loadingClassName="rounded-full"
      icon={icon}
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
