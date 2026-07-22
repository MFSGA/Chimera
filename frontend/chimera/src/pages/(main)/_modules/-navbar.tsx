import { commands, useClashProxies, useSetting } from '@chimera/interface';
import { cn } from '@chimera/utils';
import {
  Apps,
  DashboardRounded,
  DesignServicesRounded,
  ExitToAppRounded,
  GridViewRounded,
  MenuRounded,
  Public,
  SettingsEthernetRounded,
  SettingsRounded,
  TerminalRounded,
} from '@mui/icons-material';
import { Link, useMatchRoute } from '@tanstack/react-router';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import { ComponentProps, ReactNode, useMemo } from 'react';
import AnimatedTabs, { AnimatedTabsItem } from '@/components/ui/animated-tabs';
import { Button } from '@/components/ui/button';
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu';
import { useLockFn } from '@/hooks/use-lock-fn';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const currentWindow = getCurrentWebviewWindow();

function NavbarButton({
  to,
  params,
  children,
}: {
  to: string;
  params?: Record<string, string>;
  children: ReactNode;
}) {
  const matchRoute = useMatchRoute();

  const isActive = !!matchRoute({
    to,
    params,
    fuzzy: true,
  } as never);

  return (
    <AnimatedTabsItem
      className={cn('md:min-w-max md:flex-none [&_svg]:size-5')}
      data-active={String(isActive)}
      data-slot="animated-tabs-item"
      isActive={isActive}
      asChild
    >
      <Link to={to as never} params={params as never}>
        {children}
      </Link>
    </AnimatedTabsItem>
  );
}

const NavbarLabel = ({ className, ...props }: ComponentProps<'span'>) => {
  return (
    <span
      className={cn('text-sm font-medium text-nowrap', className)}
      data-slot="navbar-label"
      {...props}
    />
  );
};

const MoblieNavbarContainer = ({
  className,
  ...props
}: ComponentProps<'div'>) => {
  return (
    <div
      className={cn(
        'flex min-w-0 flex-1 flex-col items-center gap-1',
        className,
      )}
      data-slot="mobile-navbar-container"
      {...props}
    />
  );
};

export const DefaultNavbar = () => {
  const {
    proxies: { data: proxies },
  } = useClashProxies();

  const fristGroup = useMemo(() => {
    return proxies?.groups[0]?.name;
  }, [proxies]);

  return (
    <AnimatedTabs
      className={cn(
        'bg-transparent!',
        '**:data-[slot=animated-tabs-indicator]:bg-inverse-primary',
        '**:dark:data-[slot=animated-tabs-indicator]:bg-primary-container',
      )}
      data-slot="app-navbar"
      variant="pill"
      size="sm"
    >
      <NavbarButton to="/main/dashboard">
        <DashboardRounded />
        <NavbarLabel>{m.navbar_label_dashboard()}</NavbarLabel>
      </NavbarButton>

      {fristGroup ? (
        <NavbarButton to="/main/proxies/$name" params={{ name: fristGroup }}>
          <Public />
          <NavbarLabel>{m.navbar_label_proxies()}</NavbarLabel>
        </NavbarButton>
      ) : (
        <NavbarButton to="/main/proxies">
          <Public />
          <NavbarLabel>{m.navbar_label_proxies()}</NavbarLabel>
        </NavbarButton>
      )}

      <NavbarButton to="/main/profiles/$type" params={{ type: 'profile' }}>
        <GridViewRounded />
        <NavbarLabel>{m.navbar_label_profiles()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/connections">
        <SettingsEthernetRounded />
        <NavbarLabel>{m.navbar_label_connections()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/rules">
        <DesignServicesRounded />
        <NavbarLabel>{m.navbar_label_rules()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/logs">
        <TerminalRounded />
        <NavbarLabel>{m.navbar_label_logs()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/settings/system">
        <SettingsRounded />
        <NavbarLabel>{m.navbar_label_settings()}</NavbarLabel>
      </NavbarButton>

      <NavbarButton to="/main/providers">
        <Apps />
        <NavbarLabel>{m.navbar_label_providers()}</NavbarLabel>
      </NavbarButton>
    </AnimatedTabs>
  );
};

export const MobileNavbar = () => {
  return (
    <AnimatedTabs
      className={cn(
        'h-full w-full bg-transparent! py-2',
        '**:data-[slot=animated-tabs-indicator]:bg-inverse-primary',
        '**:dark:data-[slot=animated-tabs-indicator]:bg-on-primary',
      )}
      variant="pill"
      size="sm"
    >
      <MoblieNavbarContainer>
        <NavbarButton to="/main/dashboard">
          <DashboardRounded />
        </NavbarButton>
        {m.navbar_label_dashboard()}
      </MoblieNavbarContainer>

      <MoblieNavbarContainer>
        <NavbarButton to="/main/proxies">
          <Public />
        </NavbarButton>
        {m.navbar_label_proxies()}
      </MoblieNavbarContainer>

      <MoblieNavbarContainer>
        <NavbarButton to="/main/connections">
          <SettingsEthernetRounded />
        </NavbarButton>
        {m.navbar_label_connections()}
      </MoblieNavbarContainer>

      <MoblieNavbarContainer>
        <NavbarButton to="/main/settings/system">
          <SettingsRounded />
        </NavbarButton>
        {m.navbar_label_settings()}
      </MoblieNavbarContainer>

      <DropdownMenu>
        <MoblieNavbarContainer>
          <DropdownMenuTrigger asChild>
            <Button
              className="min-w-0 flex-1 bg-transparent! px-4 text-current"
              variant="flat"
            >
              <MenuRounded className="size-5" />
            </Button>
          </DropdownMenuTrigger>
          {m.navbar_label_more()}
        </MoblieNavbarContainer>

        <DropdownMenuContent>
          <DropdownMenuItem asChild>
            <Link
              to={'/main/profiles/$type' as never}
              params={{ type: 'profile' } as never}
            >
              <GridViewRounded />
              <span>{m.navbar_label_profiles()}</span>
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to={'/main/rules' as never}>
              <DesignServicesRounded />
              <span>{m.navbar_label_rules()}</span>
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to={'/main/logs' as never}>
              <TerminalRounded />
              <span>{m.navbar_label_logs()}</span>
            </Link>
          </DropdownMenuItem>
          <DropdownMenuItem asChild>
            <Link to={'/main/providers' as never}>
              <Apps />
              <span>{m.navbar_label_providers()}</span>
            </Link>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </AnimatedTabs>
  );
};

export function LegacyNavbarButton() {
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
        title: m.common_error(),
      });
    }
  });

  return (
    <Button
      className="ml-auto flex h-9 items-center gap-2 px-4 [&_svg]:size-5"
      loading={windowType.isPending}
      onClick={() => void switchToLegacy()}
    >
      <ExitToAppRounded />
      <NavbarLabel>Switch to Legacy UI</NavbarLabel>
    </Button>
  );
}
