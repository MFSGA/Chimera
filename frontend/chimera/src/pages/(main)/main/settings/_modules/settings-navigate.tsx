import { useSetting } from '@chimera/interface';
import { cn } from '@chimera/ui';
import { Link, useLocation } from '@tanstack/react-router';
import DisplayExternalInput from '~icons/material-symbols/display-external-input-rounded';
import FrameBugOutlineRounded from '~icons/material-symbols/frame-bug-outline-rounded';
import SettingsBoltRounded from '~icons/material-symbols/settings-b-roll-rounded';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import SettingsRounded from '~icons/material-symbols/settings-rounded';
import ViewQuilt from '~icons/material-symbols/view-quilt-rounded';
import type { ComponentProps, ReactNode } from 'react';
import ChimeraIcon from '@root/backend/tauri/icons/icon.png';
import { getImage } from '@/components/setting/modules/clash-core';
import { Button } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';

const ChimeraLogo = () => (
  <img
    src={ChimeraIcon}
    className="size-8 rounded-full object-cover"
    alt=""
    data-slot="chimera-logo"
  />
);

const CurrentCoreIcon = ({
  className,
  ...props
}: Omit<ComponentProps<'img'>, 'src'>) => {
  const { value } = useSetting('clash_core');

  return (
    <img
      src={getImage(value ?? 'mihomo')}
      className={cn('size-full', className)}
      {...props}
    />
  );
};

const NavigateButton = ({
  icon,
  label,
  description,
  className,
  ...props
}: ComponentProps<typeof Link> & {
  icon: ReactNode;
  label: string;
  description: string;
}) => {
  const location = useLocation();
  const isActive = location.pathname === props.to;

  return (
    <Button
      variant="fab"
      data-active={String(isActive)}
      className={cn(
        'h-16',
        'flex items-center gap-2',
        'data-[active=true]:bg-surface-variant/80',
        'data-[active=false]:bg-transparent',
        'data-[active=false]:shadow-none',
        'data-[active=false]:hover:shadow-none',
        'data-[active=false]:hover:bg-surface-variant/30',
        className,
      )}
      asChild
    >
      <Link {...props}>
        <div className="flex max-w-full items-center gap-3">
          <div className="size-8">{icon}</div>

          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <div className="text-sm font-medium">{label}</div>

            <TextMarquee className="text-xs text-zinc-500">
              {description}
            </TextMarquee>
          </div>
        </div>
      </Link>
    </Button>
  );
};

const SystemButton = () => (
  <NavigateButton
    icon={<SettingsEthernet className="size-8" />}
    label={m.settings_label_system()}
    description={m.settings_label_system_description()}
    to="/main/settings/system"
  />
);

const UserInterfaceButton = () => (
  <NavigateButton
    icon={<ViewQuilt className="size-8" />}
    label={m.settings_label_user_interface()}
    description={m.settings_label_user_interface_description()}
    to="/main/settings/user-interface"
  />
);

const ClashButton = () => (
  <NavigateButton
    icon={<SettingsBoltRounded className="size-8" />}
    label={m.settings_label_clash_settings()}
    description={m.settings_label_clash_settings_description()}
    to="/main/settings/clash"
  />
);

const ExternalControllButton = () => (
  <NavigateButton
    icon={
      <div className="relative size-8">
        <CurrentCoreIcon className="size-7.5" />
        <div
          className={cn(
            'absolute -right-1 -bottom-1 size-4 rounded-full p-0.5 shadow-sm',
            'bg-surface-variant text-primary',
          )}
        >
          <DisplayExternalInput className="size-3" />
        </div>
      </div>
    }
    label={m.settings_label_external_controll()}
    description={m.settings_label_external_controll_description()}
    to="/main/settings/web-ui"
  />
);

const NyanpasuButton = () => (
  <NavigateButton
    icon={
      <div className="relative size-8">
        <ChimeraLogo />
        <div
          className={cn(
            'absolute -right-1 -bottom-1 size-4 rounded-full p-0.5 shadow-sm',
            'bg-surface-variant text-primary',
          )}
        >
          <SettingsRounded className="text-primary size-3" />
        </div>
      </div>
    }
    label={m.settings_label_nyanpasu()}
    description={m.settings_label_nyanpasu_description()}
    to="/main/settings/nyanpasu"
  />
);

const DebugButton = () => (
  <NavigateButton
    icon={<FrameBugOutlineRounded className="size-8" />}
    label={m.settings_label_debug()}
    description={m.settings_label_debug_description()}
    to="/main/settings/debug"
  />
);

const AboutButton = () => (
  <NavigateButton
    icon={<ChimeraLogo />}
    label={m.settings_label_about()}
    description={m.settings_label_about_description()}
    to="/main/settings/about"
  />
);

export default function SettingsNavigate() {
  return (
    <div className="flex flex-col gap-2 p-2">
      <SystemButton />
      <UserInterfaceButton />
      <ClashButton />
      <ExternalControllButton />
      <NyanpasuButton />
      <DebugButton />
      <AboutButton />
    </div>
  );
}
