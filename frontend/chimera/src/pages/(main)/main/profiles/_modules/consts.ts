/**
 * Profiles 模块共享常量
 *
 * 迁移自 ref: `src/pages/(main)/main/profiles/_modules/consts.ts`
 *
 * 职责：
 * - 定义 ProfileType 枚举（Profile / JavaScript / Lua / Merge）
 * - 定义每种 Profile 类型对应的过滤条件
 * - 提供类型名称映射
 *
 * 被 ProfilesNavigate 和 $type 子路由共享
 */

import * as m from '@/paraglide/messages';

/**
 * Profile 类型枚举
 * 用于按配置类型过滤和导航
 */
export enum ProfileType {
  Profile = 'profile',
  JavaScript = 'javascript',
  Lua = 'lua',
  Merge = 'merge',
}

/**
 * Profile 类型到显示名称的映射
 */
export const PROFILE_TYPE_NAMES = {
  [ProfileType.Profile]: m.profile_profile_label(),
  [ProfileType.JavaScript]: m.profile_javascript_label(),
  [ProfileType.Lua]: m.profile_lua_label(),
  [ProfileType.Merge]: m.profile_merge_label(),
} satisfies Record<ProfileType, string>;

/**
 * Profile 类型到过滤条件的映射
 * 每种类型对应一组匹配条件，用于筛选配置列表
 */
export const PROFILE_TYPE_CONDITIONS = {
  [ProfileType.Profile]: [
    { type: 'remote' as const },
    { type: 'local' as const },
  ],
  [ProfileType.JavaScript]: [
    { type: 'script' as const, script_type: 'javascript' as const },
  ],
  [ProfileType.Lua]: [{ type: 'script' as const, script_type: 'lua' as const }],
  [ProfileType.Merge]: [{ type: 'merge' as const }],
} as const;
