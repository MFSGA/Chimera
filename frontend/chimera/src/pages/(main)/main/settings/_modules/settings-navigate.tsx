import {
  NavigationList,
  NavigationListGroup,
  NavigationListItem,
  NavigationListItemIcon,
  NavigationListItemText,
  type NavigationListItemColor,
} from '@chimera/ui/navigation-list';
import { Link, useLinkProps } from '@tanstack/react-router';
import DataObjectRounded from '~icons/material-symbols/data-object-rounded';
import DeployedCodeRounded from '~icons/material-symbols/deployed-code-rounded';
import FrameBugOutlineRounded from '~icons/material-symbols/frame-bug-outline-rounded';
import LanRounded from '~icons/material-symbols/lan-rounded';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import SettingsRounded from '~icons/material-symbols/settings-rounded';
import SwapHorizRounded from '~icons/material-symbols/swap-horiz-rounded';
import TuneRounded from '~icons/material-symbols/tune-rounded';
import ViewQuilt from '~icons/material-symbols/view-quilt-rounded';
import type { ComponentProps, ForwardedRef, ReactNode } from 'react';
import ChimeraIcon from '@root/backend/tauri/icons/icon.png';
import * as m from '@/paraglide/messages';

const ChimeraLogo = () => (
  <img src={ChimeraIcon} className="size-8 rounded-full object-cover" alt="" />
);

function SettingsNavigationItem({
  icon,
  color,
  image,
  label,
  description,
  ref,
  ...props
}: ComponentProps<typeof Link> & {
  icon: ReactNode;
  color?: NavigationListItemColor;
  image?: boolean;
  label: string;
  description: string;
}) {
  const linkProps = useLinkProps(
    {
      ...props,
      activeOptions: { ...props.activeOptions, includeSearch: false },
    } as Parameters<typeof useLinkProps>[0],
    ref as ForwardedRef<Element>,
  );
  const isActive = linkProps['aria-current'] === 'page';

  return (
    <NavigationListItem active={isActive} asChild>
      <a {...linkProps}>
        {image ? (
          <NavigationListItemIcon image>{icon}</NavigationListItemIcon>
        ) : (
          <NavigationListItemIcon color={color ?? 'neutral'}>
            {icon}
          </NavigationListItemIcon>
        )}
        <NavigationListItemText headline={label} supporting={description} />
      </a>
    </NavigationListItem>
  );
}

export default function SettingsNavigate() {
  return (
    <NavigationList aria-label={m.navbar_label_settings()}>
      <NavigationListGroup label={m.settings_label_system()}>
        <SettingsNavigationItem
          to="/main/settings/system"
          color="blue"
          icon={<SettingsEthernet />}
          label={m.settings_label_system()}
          description={m.settings_label_system_description()}
        />
      </NavigationListGroup>
      <NavigationListGroup label={m.settings_label_clash_settings()}>
        <SettingsNavigationItem
          to="/main/settings/clash/core-manager"
          color="purple"
          icon={<DeployedCodeRounded />}
          label={m.settings_label_core_manager()}
          description={m.settings_label_core_manager_description()}
        />
        <SettingsNavigationItem
          to="/main/settings/clash/core"
          color="orange"
          icon={<TuneRounded />}
          label={m.settings_label_core_settings()}
          description={m.settings_label_core_settings_description()}
        />
        <SettingsNavigationItem
          to="/main/settings/clash/network"
          color="cyan"
          icon={<LanRounded />}
          label={m.settings_label_network()}
          description={m.settings_label_network_description()}
        />
        <SettingsNavigationItem
          to="/main/settings/clash/control"
          color="blue"
          icon={<SwapHorizRounded />}
          label={m.settings_label_control()}
          description={m.settings_label_control_description()}
        />
        <SettingsNavigationItem
          to="/main/settings/clash/runtime"
          color="yellow"
          icon={<DataObjectRounded />}
          label={m.settings_label_runtime()}
          description={m.settings_label_runtime_description()}
        />
      </NavigationListGroup>
      <NavigationListGroup label={m.settings_label_chimera()}>
        <SettingsNavigationItem
          to="/main/settings/user-interface"
          color="pink"
          icon={<ViewQuilt />}
          label={m.settings_label_user_interface()}
          description={m.settings_label_user_interface_description()}
        />
        <SettingsNavigationItem
          to="/main/settings/chimera"
          color="green"
          icon={<SettingsRounded />}
          label={m.settings_label_chimera()}
          description={m.settings_label_chimera_description()}
        />
        <SettingsNavigationItem
          to="/main/settings/debug"
          color="orange"
          icon={<FrameBugOutlineRounded />}
          label={m.settings_label_debug()}
          description={m.settings_label_debug_description()}
        />
      </NavigationListGroup>
      <NavigationListGroup label={m.settings_label_about()}>
        <SettingsNavigationItem
          to="/main/settings/about"
          image
          icon={<ChimeraLogo />}
          label={m.settings_label_about()}
          description={m.settings_label_about_description()}
        />
      </NavigationListGroup>
    </NavigationList>
  );
}
