import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { notification, NotificationType } from '@/utils/notification';

type SetConfigPayload =
  | { ok?: string | null; Ok?: string | null }
  | { err?: string; Err?: string };

type NoticePayload = {
  set_config?: SetConfigPayload | null;
};

const getSetConfigStatus = (payload?: SetConfigPayload | null) => {
  if (!payload) {
    return null;
  }

  if ('ok' in payload || 'Ok' in payload) {
    return 'ok';
  }

  if ('err' in payload || 'Err' in payload) {
    return 'err';
  }

  return null;
};

const getSetConfigError = (payload?: SetConfigPayload | null) => {
  if (!payload) {
    return null;
  }

  const errorPayload = payload as { err?: string; Err?: string };

  return errorPayload.err ?? errorPayload.Err ?? null;
};

export const NoticeProvider = () => {
  const { t } = useTranslation();
  const unlistenRef = useRef<UnlistenFn | null>(null);

  useEffect(() => {
    listen<NoticePayload>('nyanpasu://notice-message', ({ payload }) => {
      const setConfigStatus = getSetConfigStatus(payload?.set_config);

      if (setConfigStatus === 'ok') {
        notification({
          title: t('Successful'),
          body: t('Refresh Clash Config'),
          type: NotificationType.Success,
        }).catch((error) => {
          console.error(error);
        });
        return;
      }

      if (setConfigStatus === 'err') {
        notification({
          title: t('Error'),
          body: getSetConfigError(payload?.set_config) ?? undefined,
          type: NotificationType.Error,
        }).catch((error) => {
          console.error(error);
        });
      }
    })
      .then((unlisten) => {
        unlistenRef.current = unlisten;
      })
      .catch((error) => {
        notification({
          title: t('Error'),
          body: error.message,
          type: NotificationType.Error,
        }).catch((notificationError) => {
          console.error(notificationError);
        });
      });

    return () => {
      unlistenRef.current?.();
    };
  }, [t]);

  return null;
};

export default NoticeProvider;
