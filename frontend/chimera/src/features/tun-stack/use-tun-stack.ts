import {
  useRuntimeProfile,
  useSetting,
  type TunStack,
} from '@chimera/interface';
import { useLockFn } from 'ahooks';
import { useMemo, useState } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';

const TUN_STACK_OPTIONS = {
  system: 'System',
  gvisor: 'gVisor',
  mixed: 'Mixed',
} satisfies Record<TunStack, string>;

export const getTunStackOptions = (
  coreType: string | null | undefined,
): Partial<Record<TunStack, string>> => {
  if (coreType === 'clash') {
    return {
      system: TUN_STACK_OPTIONS.system,
      gvisor: TUN_STACK_OPTIONS.gvisor,
    };
  }

  return TUN_STACK_OPTIONS;
};

/** Keep TUN stack changes serialized and refresh every runtime view they affect. */
export const useTunStackAction = () => {
  const tunStack = useSetting('tun_stack');
  const enableTunMode = useSetting('enable_tun_mode');
  const runtimeProfile = useRuntimeProfile();
  const [isPending, setIsPending] = useState(false);

  const execute = useLockFn(async (value: TunStack) => {
    setIsPending(true);
    try {
      await tunStack.upsert(value);

      if (enableTunMode.value) {
        await enableTunMode.upsert(true);
      }

      await runtimeProfile.refetch();
    } catch (error) {
      message(
        `${m.settings_clash_tun_stack_change_failed()}\n${formatError(error)}`,
        { title: m.common_error(), kind: 'error' },
      );
    } finally {
      setIsPending(false);
    }
  });

  return {
    execute,
    isPending,
    value: tunStack.value,
  };
};

export const useTunStackModel = (coreType: string | null | undefined) => {
  const action = useTunStackAction();
  const options = useMemo(() => getTunStackOptions(coreType), [coreType]);
  const selected = useMemo(() => {
    const stack = action.value || 'gvisor';
    return stack in options ? stack : 'gvisor';
  }, [action.value, options]);

  return { ...action, options, selected };
};
