import { useMutation } from '@tanstack/react-query';
import { useEffect, useRef } from 'react';
import { unwrapResult } from '../utils';
import { commands } from './bindings';
import { useClashSetting, useSetting } from './use-settings';

/** Single opt-in probe, never a polling query or an unguarded browser fetch. */
export function useLocalIpProbe(enabled = true) {
  const { value: allowed } = useSetting('enable_local_ip_probe');
  const { value: tunEnabled } = useClashSetting('enable_tun_mode');
  const { value: transparentProxy } = useClashSetting('transparent_proxy');
  const localCapture = Boolean(
    transparentProxy?.local &&
    transparentProxy.mode != null &&
    transparentProxy.mode !== 'disabled',
  );

  const mutation = useMutation({
    mutationKey: ['probeDirectEgress'],
    mutationFn: async () => unwrapResult(await commands.probeDirectEgress()),
    retry: false,
  });
  const { mutate } = mutation;
  const probed = useRef(false);

  useEffect(() => {
    if (!enabled || allowed !== true || tunEnabled !== false || localCapture) {
      probed.current = false;
      return;
    }
    if (!probed.current) {
      probed.current = true;
      mutate();
    }
  }, [enabled, allowed, tunEnabled, localCapture, mutate]);

  return mutation;
}
