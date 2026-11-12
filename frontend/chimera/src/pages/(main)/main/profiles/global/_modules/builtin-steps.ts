import type { BuiltinStepKind } from '@chimera/interface';
import * as m from '@/paraglide/messages';

/**
 * Labels of the built-in steps Chimera runs, in pipeline order. Keep this
 * exhaustive so a new runtime step cannot silently disappear from the UI.
 * The core-controller snapshot is appended by Inspect, not a setting.
 */
export const BUILTIN_STEP_LABELS = {
  guard_overrides: () => m.inspect_tag_guard(),
  whitelist_field_filter: () => m.inspect_tag_filter(),
  keep_unmanaged: () => m.inspect_tag_keep_unmanaged(),
  managed_settings: () => m.inspect_tag_managed_settings(),
  tun_keep_unmanaged: () => m.inspect_tag_tun_keep_unmanaged(),
  tun_managed_settings: () => m.inspect_tag_tun_managed_settings(),
  final_field_filter: () => m.inspect_tag_final_filter(),
  defaults: () => m.inspect_tag_defaults(),
  include_all_expansion: () => m.inspect_tag_include_all(),
  finalizing: () => m.inspect_tag_finalizing(),
} satisfies Record<Exclude<BuiltinStepKind, 'core_controller'>, () => string>;
