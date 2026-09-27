import assert from 'node:assert/strict';
import type {
  MutationOutcome,
  NewProfileRequest_Deserialize,
  ProfileDefinition_Deserialize,
  ProfileDocument_Deserialize,
  ProfileItem_Deserialize,
  ScriptRuntime,
} from '../../frontend/interface/src/ipc/bindings.js';

export type CleanupAction = { label: string; run: () => Promise<unknown> };

export type CleanupRegistrar = (
  label: string,
  action: () => Promise<unknown>,
) => void;

export async function withCleanup<T>(
  name: string,
  action: (defer: CleanupRegistrar) => Promise<T>,
): Promise<T> {
  const cleanups: Array<{ label: string; action: () => Promise<unknown> }> = [];
  const defer: CleanupRegistrar = (label, cleanup) => {
    cleanups.push({ label, action: cleanup });
  };
  let value!: T;
  let primaryError: unknown;
  let hasPrimaryError = false;

  try {
    value = await action(defer);
  } catch (error) {
    primaryError = error;
    hasPrimaryError = true;
  }

  const cleanupErrors: Error[] = [];
  for (const cleanup of cleanups) {
    try {
      await cleanup.action();
    } catch (error) {
      cleanupErrors.push(
        new Error(`${name}: ${cleanup.label} failed`, { cause: error }),
      );
    }
  }

  if (hasPrimaryError && cleanupErrors.length === 0) throw primaryError;
  if (hasPrimaryError || cleanupErrors.length > 0) {
    throw new AggregateError(
      [...(hasPrimaryError ? [primaryError] : []), ...cleanupErrors],
      `${name} failed${cleanupErrors.length ? ' and cleanup was incomplete' : ''}`,
    );
  }
  return value;
}

export async function runCleanupActions(
  name: string,
  actions: CleanupAction[],
): Promise<void> {
  const errors: Error[] = [];
  for (const action of actions) {
    try {
      await action.run();
    } catch (cause) {
      errors.push(new Error(`${name}: ${action.label} failed`, { cause }));
    }
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, `${name} cleanup was incomplete`);
  }
}

export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return browser.execute(
    async (name, parameters) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__: {
            invoke: <R>(
              command: string,
              args?: Record<string, unknown>,
            ) => Promise<R>;
          };
        }
      ).__TAURI_INTERNALS__;
      return internals.invoke<T>(name, parameters);
    },
    command,
    args,
  );
}

export function localConfigProfileRequest(
  name: string,
): NewProfileRequest_Deserialize {
  return {
    metadata: { name, desc: null },
    definition: {
      type: 'config',
      config: {
        type: 'file',
        source: {
          type: 'local',
          binding: { type: 'managed', file: 'pending.yaml' },
        },
        transforms: [],
      },
    },
  };
}

export function overlayProfileRequest(
  name: string,
): NewProfileRequest_Deserialize {
  return {
    metadata: { name, desc: null },
    definition: {
      type: 'transform',
      transform: {
        type: 'overlay',
        source: {
          type: 'local',
          binding: { type: 'managed', file: 'pending.yaml' },
        },
      },
    },
  };
}

export function scriptProfileRequest(
  name: string,
  runtime: ScriptRuntime,
): NewProfileRequest_Deserialize {
  return {
    metadata: { name, desc: null },
    definition: {
      type: 'transform',
      transform: {
        type: 'script',
        source: {
          type: 'local',
          binding: {
            type: 'managed',
            file: runtime === 'javascript' ? 'pending.js' : 'pending.lua',
          },
        },
        runtime,
      },
    },
  };
}

export async function createProfile(
  request: NewProfileRequest_Deserialize,
  fileData: string | null,
): Promise<MutationOutcome<string>> {
  return invoke('create_profile', { request, fileData });
}

export function committedValue<T>(
  outcome: MutationOutcome<T>,
  operation: string,
): T {
  assert.equal(
    outcome.status,
    'committed',
    `${operation} degraded: ${
      outcome.status === 'committed_degraded'
        ? outcome.degradations
            .map((item) => `${item.code}: ${item.message}`)
            .join('; ')
        : 'unknown outcome'
    }`,
  );
  return outcome.value;
}

export function appliedValue<T>(
  outcome: MutationOutcome<T>,
  operation: string,
): T {
  const value = committedValue(outcome, operation);
  assert.ok(
    outcome.commits.some((commit) => commit.runtime === 'applied'),
    `${operation} committed without an applied Profile runtime receipt.`,
  );
  return value;
}

export async function readProfiles(): Promise<ProfileDocument_Deserialize> {
  return invoke('get_profiles');
}

export async function findProfileUid(
  name: string,
): Promise<string | undefined> {
  return (await readProfiles()).items.find((item) => item.name === name)?.uid;
}

export async function replaceProfileDefinition(
  uid: string,
  definition: ProfileDefinition_Deserialize,
): Promise<MutationOutcome<null>> {
  return invoke('replace_profile_definition', { uid, definition });
}

export async function setScopedTransforms(
  uid: string,
  transforms: string[],
): Promise<MutationOutcome<null>> {
  const profiles = await readProfiles();
  const item = profiles.items.find((profile) => profile.uid === uid);
  if (!item || item.type !== 'config') {
    throw new Error(`Config profile ${uid} is unavailable.`);
  }

  return replaceProfileDefinition(uid, {
    type: 'config',
    config: { ...item.config, transforms },
  });
}

export async function setGlobalTransforms(
  ids: string[],
): Promise<MutationOutcome<null>> {
  return invoke('set_global_transforms', { ids });
}

export async function activateProfile(
  uid: string | null,
): Promise<MutationOutcome<null>> {
  return invoke('activate_profile', { uid });
}

export async function deleteProfile(
  uid: string,
): Promise<MutationOutcome<null>> {
  return invoke('delete_profile', { uid });
}

export function scopedTransformsOf(item: ProfileItem_Deserialize): string[] {
  return item.type === 'config' ? (item.config.transforms ?? []) : [];
}
