import { kvStorageDebug, useSetting } from '@chimera/interface';
import { listen } from '@tauri-apps/api/event';
import { useAtom } from 'jotai';
import { useEffect } from 'react';
import {
  applyStorageSnapshot,
  dispatchStorageValueChanged,
} from '@/services/storage';
import { createStorageListenerSubscription } from '@/services/storage-listeners';
import { createStorageResyncCoordinator } from '@/services/storage-resync';
import { coreTypeAtom } from '@/store/clash';

export function useCoreType() {
  const [coreType, setCoreType] = useAtom(coreTypeAtom);

  const { upsert } = useSetting('clash_core');

  const setter = (value: typeof coreType) => {
    setCoreType(value);
    upsert(value);
  };
  return [coreType, setter] as const;
}

export function useNyanpasuStorageSubscribers() {
  useEffect(() => {
    const storageResync = createStorageResyncCoordinator(
      () => kvStorageDebug.getAll(),
      applyStorageSnapshot,
      (error) => {
        console.error('[storage] failed to resync complete snapshot:', error);
      },
    );
    const storageListeners = createStorageListenerSubscription({
      listen,
      onStorageValueChanged: ([key, value]) => {
        dispatchStorageValueChanged(
          key,
          typeof value === 'string' ? JSON.parse(value) : value,
        );
      },
      onStorageResyncRequired: () => {
        void storageResync.resync();
      },
      onRegistrationError: (event, error) => {
        console.error(`[storage] failed to register ${event} listener:`, error);
      },
      onEventError: (event, error) => {
        console.error(`[storage] failed to handle ${event} event:`, error);
      },
    });

    return () => {
      storageListeners.dispose();
      storageResync.dispose();
    };
  }, []);
}
