import { cn } from '@chimera/ui';
import {
  Apps,
  DashboardRounded,
  DesignServicesRounded,
  GridViewRounded,
  Public,
  SettingsEthernetRounded,
  SettingsRounded,
  TerminalRounded,
} from '@mui/icons-material';
import { Tooltip } from '@mui/material';
import { Link, useLocation } from '@tanstack/react-router';
import { ComponentProps, ReactNode } from 'react';
import { Button } from '@/components/ui/button';

const navItems = [
  {
    to: '/main',
    activeWhen: '/main',
    label: 'Dashboard',
    icon: <DashboardRounded />,
  },
  {
    to: '/main',
    activeWhen: '/main/proxies',
    label: 'Proxies',
    icon: <Public />,
  },
  {
    to: '/main',
    activeWhen: '/main/profiles',
    label: 'Profiles',
    icon: <GridViewRounded />,
  },
  {
    to: '/main',
    activeWhen: '/main/connections',
    label: 'Connections',
    icon: <SettingsEthernetRounded />,
  },
  {
    to: '/main',
    activeWhen: '/main/rules',
    label: 'Rules',
    icon: <DesignServicesRounded />,
  },
  {
    to: '/main',
    activeWhen: '/main/logs',
    label: 'Logs',
    icon: <TerminalRounded />,
  },
  {
    to: '/main',
    activeWhen: '/main/settings',
    label: 'Settings',
    icon: <SettingsRounded />,
  },
  {
    to: '/main',
    activeWhen: '/main/providers',
    label: 'Providers',
    icon: <Apps />,
  },
] as const;

const NavbarButton = ({
  icon,
  label,
  to,
  activeWhen,
}: {
  icon: ReactNode;
  label: string;
  to: ComponentProps<typeof Link>['to'];
  activeWhen: string;
}) => {
  const location = useLocation();
  const isActive =
    activeWhen === '/main'
      ? location.pathname === activeWhen
      : location.pathname.startsWith(activeWhen);

  return (
    <Tooltip title={label} placement="top">
      <Button
        className={cn(
          'flex min-w-0 items-center justify-center gap-1',
          'h-10 px-3 sm:h-8',
          'hover:bg-primary-container dark:hover:bg-primary-container',
          'data-[active=true]:bg-inverse-primary dark:data-[active=true]:bg-primary-container',
          'lg:w-fit',
        )}
        data-active={String(isActive)}
        asChild
      >
        <Link to={to}>
          <span className="size-5 [&>svg]:size-5" data-slot="navbar-icon">
            {icon}
          </span>

          <span className="hidden lg:block" data-slot="navbar-label">
            {label}
          </span>
        </Link>
      </Button>
    </Tooltip>
  );
};

export default function Navbar({ className, ...props }: ComponentProps<'div'>) {
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
