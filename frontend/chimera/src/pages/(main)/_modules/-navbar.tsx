import { commands, useSetting } from '@chimera/interface';
import { cn } from '@chimera/ui';
import {
  Apps,
  DashboardRounded,
  DesignServicesRounded,
  ExitToAppRounded,
  GridViewRounded,
  Public,
  SettingsEthernetRounded,
  SettingsRounded,
  TerminalRounded,
} from '@mui/icons-material';
import { Tooltip } from '@mui/material';
import { Link, useLocation } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ComponentProps, ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();

const NavbarButton = ({
  icon,
  label,
  to,
  activeWhen,
  onClick,
  loading,
  pushRight,
}: {
  icon: ReactNode;
  label: string;
  to?: ComponentProps<typeof Link>['to'];
  activeWhen?: string;
  onClick?: () => void;
  loading?: boolean;
  pushRight?: boolean;
}) => {
  const location = useLocation();
  const isActive = activeWhen
    ? activeWhen === '/main'
      ? location.pathname === activeWhen
      : location.pathname.startsWith(activeWhen)
    : false;

  const content = (
    <>
      <span className="size-5 [&>svg]:size-5" data-slot="navbar-icon">
        {icon}
      </span>

      <span className="hidden lg:block" data-slot="navbar-label">
        {label}
      </span>
    </>
  );

  return (
    <Tooltip title={label} placement="top">
      <Button
        className={cn(
          'flex min-w-0 items-center justify-center gap-1',
          'h-10 px-3 sm:h-8',
          'hover:bg-primary-container dark:hover:bg-primary-container',
          'data-[active=true]:bg-inverse-primary dark:data-[active=true]:bg-primary-container',
          'lg:w-fit',
          pushRight && 'ml-auto',
        )}
        data-active={String(isActive)}
        asChild={Boolean(to)}
        loading={loading}
        onClick={onClick}
        aria-label={label}
      >
        {to ? <Link to={to}>{content}</Link> : content}
      </Button>
    </Tooltip>
  );
};

export default function Navbar({ className, ...props }: ComponentProps<'div'>) {
  const windowType = useSetting('window_type');

  const switchToLegacy = useLockFn(async () => {
    try {
      await windowType.upsert('legacy');

      const result = await commands.createLegacyWindow();

      if (result.status !== 'ok') {
        throw new Error(result.error);
      }

      await currentWindow.close();
    } catch (error) {
      await message(`Failed to open legacy UI: ${formatError(error)}`, {
        kind: 'error',
        title: 'Error',
      });
    }
  });

  /**
   * 导航栏项目列表
   *
   * 迁移自 ref: `src/pages/(main)/_modules/navbar.tsx`
   *
   * 路由映射：
   * - Dashboard → `/main/dashboard`（着陆页）
   * - Proxies → `/main/proxies`（已迁移）
   * - Profiles → `/main/profiles`（已迁移）
   * - Connections → `/main/connections`（已迁移）
   * - Rules → `/main/rules`（已迁移）
   * - Logs → `/main/logs`（已迁移）
   * - Settings → `/main/settings`（已迁移）
   * - Providers → `/main/providers`（已迁移）
   *
   * TODO: 页面迁移完成后，移除到 legacy UI 的切换按钮
   */
  const navItems = [
    {
      to: '/main/dashboard',
      activeWhen: '/main/dashboard',
      label: 'Dashboard',
      icon: <DashboardRounded />,
    },
    {
      to: '/main/proxies',
      activeWhen: '/main/proxies',
      label: 'Proxies',
      icon: <Public />,
    },
    {
      to: '/main/profiles',
      activeWhen: '/main/profiles',
      label: 'Profiles',
      icon: <GridViewRounded />,
    },
    {
      to: '/main/connections',
      activeWhen: '/main/connections',
      label: 'Connections',
      icon: <SettingsEthernetRounded />,
    },
    {
      to: '/main/rules',
      activeWhen: '/main/rules',
      label: 'Rules',
      icon: <DesignServicesRounded />,
    },
    {
      to: '/main/logs',
      activeWhen: '/main/logs',
      label: 'Logs',
      icon: <TerminalRounded />,
    },
    {
      to: '/main/settings',
      activeWhen: '/main/settings',
      label: 'Settings',
      icon: <SettingsRounded />,
    },
    {
      to: '/main/providers',
      activeWhen: '/main/providers',
      label: 'Providers',
      icon: <Apps />,
    },
    {
      label: 'Switch to Legacy UI',
      icon: <ExitToAppRounded />,
      onClick: () => void switchToLegacy(),
      loading: windowType.isPending,
      pushRight: true,
    },
  ] satisfies ComponentProps<typeof NavbarButton>[];

  return (
    <div
      className={cn(
        'flex h-16 shrink-0 items-center justify-between gap-2 px-3',
        'bg-primary-container dark:bg-on-primary',
        'sm:h-12 sm:justify-start',
        className,
      )}
      data-slot="app-navbar"
      {...props}
    >
      {navItems.map((item) => (
        <NavbarButton key={item.label} {...item} />
      ))}
    </div>
  );
}
