/**
 * 设置页面侧边栏导航
 *
 * 迁移自 ref: `src/pages/(main)/main/settings/_modules/settings-navigate.tsx`
 *
 * 职责：
 * - 在设置页面左侧侧边栏显示导航按钮列表
 * - 每个按钮对应一个设置子路由（系统、界面、Clash、Web UI、Nyanpasu、调试、关于）
 * - 使用 Button asChild + TanStack Router Link 实现导航
 * - 使用 TextMarquee 显示描述文字（溢出时滚动）
 * - 按钮高亮当前路由（根据 location.pathname 匹配）
 *
 * 适配说明：
 * - 移除了 ref 的 LogoSvg（Chimera 无对应资源），使用内联 SVG 替代
 * - 移除了 ref 的 useCurrentCoreIcon（Chimera 无对应 hook），使用 Settings 图标替代
 * - 使用 Chimera 的 TextMarquee 组件（与 ref 功能一致）
 * - 使用 Button 组件 variant="fab" + asChild 模式（支持）
 */

import DisplayExternalInput from '~icons/material-symbols/display-external-input-rounded';
import FrameBugOutlineRounded from '~icons/material-symbols/frame-bug-outline-rounded';
import SettingsBoltRounded from '~icons/material-symbols/settings-b-roll-rounded';
import SettingsEthernet from '~icons/material-symbols/settings-ethernet-rounded';
import SettingsRounded from '~icons/material-symbols/settings-rounded';
import ViewQuilt from '~icons/material-symbols/view-quilt-rounded';
import type { ComponentProps, ReactNode } from 'react';
import { Button } from '@/components/ui/button';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';
import { cn } from '@chimera/ui';
import { Link, useLocation } from '@tanstack/react-router';

/**
 * Chimera Logo（内联 SVG）
 * 替代 ref 的 LogoSvg 导入，使用标准 Material Symbols 图标加品牌色
 */
const ChimeraLogo = () => {
  return (
    <svg
      className="size-8"
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      data-slot="chimera-logo"
    >
      {/* 圆形背景 */}
      <circle cx="12" cy="12" r="10" className="fill-surface" />
      {/* C 形图标（Chimera 首字母） */}
      <path
        d="M12 4C7.58 4 4 7.58 4 12s3.58 8 8 8 8-3.58 8-8-3.58-8-8-8zm0 14c-3.31 0-6-2.69-6-6s2.69-6 6-6 6 2.69 6 6-2.69 6-6 6z"
        className="fill-primary"
      />
      <path
        d="M12 8c-2.21 0-4 1.79-4 4s1.79 4 4 4 4-1.79 4-4-1.79-4-4-4zm0 6c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2z"
        className="fill-primary"
      />
    </svg>
  );
};

/**
 * 导航按钮组件
 * 使用 Button asChild + Link 实现无样式丢失的路由导航
 * 自动根据当前路径高亮活动状态
 */
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
          {/* 图标区域 */}
          <div className="size-8">{icon}</div>

          {/* 标签 + 描述（文字溢出时自动滚动） */}
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

/* ========== 各子路由按钮 ========== */

/** 系统设置 */
const SystemButton = () => {
  return (
    <NavigateButton
      icon={<SettingsEthernet className="size-8" />}
      label={m.settings_label_system()}
      description={m.settings_label_system_description()}
      to="/main/settings/system"
    />
  );
};

/** 界面设置 */
const UserInterfaceButton = () => {
  return (
    <NavigateButton
      icon={<ViewQuilt className="size-8" />}
      label={m.settings_label_user_interface()}
      description={m.settings_label_user_interface_description()}
      to="/main/settings/user-interface"
    />
  );
};

/** Clash 核心设置 */
const ClashButton = () => {
  return (
    <NavigateButton
      icon={<SettingsBoltRounded className="size-8" />}
      label={m.settings_label_clash_settings()}
      description={m.settings_label_clash_settings_description()}
      to="/main/settings/clash"
    />
  );
};

/** Web UI / 外部控制设置 */
const ExternalControllButton = () => {
  return (
    <NavigateButton
      icon={
        <div className="relative size-8">
          {/* 核心图标占位 — Chimera 无 useCurrentCoreIcon，使用 Settings 图标替代 */}
          <SettingsRounded className="size-full" />

          {/* 外部控制小角标 */}
          <div
            className={cn(
              'absolute -right-1 -bottom-1 size-4 p-0.5',
              'text-primary bg-surface-variant rounded-full shadow-sm',
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
};

/** Nyanpasu（Chimera 专用）设置 */
const NyanpasuButton = () => {
  return (
    <NavigateButton
      icon={
        <div className="relative size-8">
          <ChimeraLogo />

          <div
            className={cn(
              'absolute -right-1 -bottom-1 size-4 p-0.5',
              'text-primary bg-surface-variant rounded-full shadow-sm',
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
};

/** 调试设置 */
const DebugButton = () => {
  return (
    <NavigateButton
      icon={<FrameBugOutlineRounded className="size-8" />}
      label={m.settings_label_debug()}
      description={m.settings_label_debug_description()}
      to="/main/settings/debug"
    />
  );
};

/** 关于页面 */
const AboutButton = () => {
  return (
    <NavigateButton
      icon={<ChimeraLogo />}
      label={m.settings_label_about()}
      description={m.settings_label_about_description()}
      to="/main/settings/about"
    />
  );
};

/**
 * SettingsNavigate — 设置页面侧边栏导航组件
 *
 * 渲染 7 个导航按钮：系统、界面、Clash、Web UI、Nyanpasu、调试、关于
 * 排列方式：垂直列表，间距 gap-2
 */
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
