import type { OperatorTag } from '@chimera/interface';
import * as m from '@/paraglide/messages';
import { FINAL_KEY, type Stage, type Step } from './inspect-steps';

export function stageLabel(stage: Stage): string {
  switch (stage) {
    case 'source':
      return m.inspect_stage_source();
    case 'profile':
      return m.inspect_stage_profile();
    case 'global':
      return m.inspect_stage_global();
    case 'chimera':
      return m.inspect_stage_chimera();
    case 'result':
      return m.inspect_stage_result();
  }
}

export function stepSubject(tag: OperatorTag): string | undefined {
  switch (tag.kind) {
    case 'file_config_root':
    case 'composition_root':
      return tag.data.profile_id;
    case 'extend_proxies_step':
      return tag.data.contributor_profile_id;
    case 'scoped_transform':
    case 'global_transform':
      return tag.data.transform_profile_id;
    default:
      return undefined;
  }
}

function stepKind(tag: OperatorTag): string {
  switch (tag.kind) {
    case 'bare_root':
      return m.inspect_tag_bare();
    case 'file_config_root':
      return m.inspect_tag_file();
    case 'composition_root':
      return m.inspect_tag_composition();
    case 'extend_proxies_step':
      return m.inspect_tag_extend();
    case 'scoped_transform':
      return m.inspect_tag_scoped();
    case 'global_transform':
      return m.inspect_tag_global();
    case 'builtin_transform':
      return tag.data.name || m.inspect_tag_builtin();
    case 'builtin_step':
      switch (tag.data.step) {
        case 'guard_overrides':
          return m.inspect_tag_guard();
        case 'whitelist_field_filter':
          return m.inspect_tag_filter();
        case 'keep_unmanaged':
          return m.inspect_tag_keep_unmanaged();
        case 'managed_settings':
          return m.inspect_tag_managed_settings();
        case 'tun_keep_unmanaged':
          return m.inspect_tag_tun_keep_unmanaged();
        case 'tun_managed_settings':
          return m.inspect_tag_tun_managed_settings();
        case 'final_field_filter':
          return m.inspect_tag_final_filter();
        case 'defaults':
          return m.inspect_tag_defaults();
        case 'finalizing':
          return m.inspect_tag_finalizing();
        case 'core_controller':
          return m.inspect_tag_core_controller();
      }
  }
}

export function stepHeadline(
  step: Step,
  profiles: ReadonlyMap<string, string>,
): string {
  if (step.key === FINAL_KEY) return m.inspect_final_config();
  const title = stepKind(step.node.tag);
  const subject = stepSubject(step.node.tag);
  if (!subject) return title;
  return `${title} · ${profiles.get(subject) ?? subject}`;
}
