import { getCoreDir, getServiceInstallPrompt } from '@chimera/interface';
import { BaseDialog, BaseDialogProps, cn } from '@chimera/ui';
import { useColorScheme } from '@mui/material';
import { useAtom, useSetAtom } from 'jotai';
import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import useSWR from 'swr';
import { OS } from '@/consts';
import { serviceManualPromptDialogAtom } from '@/store/service';
import styles from './service-manual-prompt-dialog.module.scss';

export type ServerManualPromptDialogProps = Omit<BaseDialogProps, 'title'> & {
  operation: 'uninstall' | 'install' | 'start' | 'stop' | null;
};

// TODO: maybe support more commands prompt?
export default function ServerManualPromptDialog({
  open,
  onClose,
  operation,
  ...props
}: ServerManualPromptDialogProps) {
  const { t } = useTranslation();
  const { mode } = useColorScheme();
  const { data: serviceInstallPrompt, error } = useSWR(
    operation === 'install' ? '/service_install_prompt' : null,
    getServiceInstallPrompt,
  );
  const { data: coreDir } = useSWR('/core_dir', () => getCoreDir());
  const commands = useMemo(() => {
    if (operation === 'install' && serviceInstallPrompt) {
      return `cd "${coreDir}"\n${serviceInstallPrompt}`;
    } else if (operation) {
      return `cd "${coreDir}"\n${OS !== 'windows' ? 'sudo ' : ''}./nyanpasu-service ${operation}`;
    }
    return '';
  }, [operation, serviceInstallPrompt, coreDir]);
  const [codes, setCodes] = useState<string | null>(null);

  /*   useAsyncEffect(async () => {
    const shiki = await getShikiSingleton();
    const code = await shiki.codeToHtml(commands, {
      lang: 'shell',
      themes: {
        dark: 'nord',
        light: 'min-light',
      },
    });
    setCodes(code);
  }, [serviceInstallPrompt, operation, coreDir, commands]);

  const handleCopyToClipboard = useCallback(() => {
    if (commands) {
      const item = new ClipboardItem({
        'text/plain': new Blob([commands], { type: 'text/plain' }),
      });
      navigator.clipboard
        .write([item])
        .then(() => {
          console.log('copied');
          notification({
            title: `Clash Nyanpasu - ${t('Service Manual Tips')}`,
            body: t('Copied to clipboard'),
          });
        })
        .catch((error) => {
          console.error(error);
          notification({
            title: `Clash Nyanpasu - ${t('Service Manual Tips')}`,
            body: t('Failed to copy to clipboard'),
          });
        });
    }
  }, [commands, t]);
 */
  return (
    <BaseDialog
      title={t('Service Manual Tips')}
      open={open}
      onClose={onClose}
      {...props}
    >
      <div className="grid gap-3">
        <p>
          {t('Unable to operation the service automatically', {
            operation: t(`${operation}`),
          })}
        </p>
        {error && <p className="text-red-500">{error.message}</p>}
        {!!codes && (
          <div className="relative">
            <div
              className={cn(
                'rounded-sm md:max-w-[80vw] lg:max-w-[60vw] xl:max-w-[50vw]',
                mode === 'dark' && styles.dark,
                styles.prompt,
              )}
              dangerouslySetInnerHTML={{
                __html: codes,
              }}
            />
            {/* todo <CopyToClipboardButton onClick={handleCopyToClipboard} /> */}
          </div>
        )}
      </div>
    </BaseDialog>
  );
}

export function ServerManualPromptDialogWrapper() {
  const [prompt, setPrompt] = useAtom(serviceManualPromptDialogAtom);
  return (
    <ServerManualPromptDialog
      open={!!prompt}
      onClose={() => setPrompt(null)}
      operation={prompt}
    />
  );
}

export function useServerManualPromptDialog() {
  const setPrompt = useSetAtom(serviceManualPromptDialogAtom);

  return {
    show: (prompt: 'install' | 'uninstall' | 'stop' | 'start') =>
      setPrompt(prompt),
    close: () => setPrompt(null),
  };
}
