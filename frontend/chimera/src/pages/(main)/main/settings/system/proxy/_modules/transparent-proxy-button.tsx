import { useSetting } from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalDescription,
  ModalTitle,
  ModalTrigger,
} from '@chimera/ui/modal';
import { Link } from '@tanstack/react-router';
import ShieldRounded from '~icons/material-symbols/shield-rounded';
import { ProxyButton } from '@/components/settings/system-proxy';
import { useTransparentProxyAction } from '@/features/system-proxy/use-proxy-settings';
import * as m from '@/paraglide/messages';

/**
 * Ported from ref's System/Proxy transparent capture shortcut. The existing
 * Chimera action retains ownership of Service checks, typed Clash patches,
 * live applied-status verification and fail-closed admission.
 */
export default function TransparentProxyButton() {
  const proxy = useTransparentProxyAction();
  const { value: serviceModeEnabled } = useSetting('enable_service_mode');

  if (!proxy.visible) return null;

  return (
    <div className="col-span-2" data-slot="settings-transparent-proxy">
      <Modal>
        <ModalTrigger
          asChild
          onClick={(event) => {
            if (!proxy.disabled) {
              event.preventDefault();
              void proxy.execute();
            }
          }}
        >
          <ProxyButton
            className="w-full"
            isActive={proxy.applied}
            loading={proxy.isPending}
            disabled={proxy.disabled && !proxy.reason}
            data-configured={String(proxy.isActive)}
            data-applied={String(proxy.applied)}
            data-slot="settings-transparent-proxy-trigger"
          >
            <ShieldRounded />
            <span>{m.settings_clash_transparent_proxy_label()}</span>
            <span className="text-sm font-normal">
              {proxy.mode === 'redir'
                ? m.settings_clash_transparent_proxy_mode_redir()
                : m.settings_clash_transparent_proxy_mode_tproxy()}
            </span>
          </ProxyButton>
        </ModalTrigger>

        <ModalContent>
          <Card className="w-96 max-w-full">
            <CardHeader>
              <ModalTitle>
                {m.settings_clash_transparent_proxy_label()}
              </ModalTitle>
            </CardHeader>
            <CardContent>
              <ModalDescription className="text-on-surface-variant text-sm">
                {proxy.reason}
              </ModalDescription>
            </CardContent>
            <CardFooter className="gap-1">
              <ModalClose asChild>
                <Button>{m.common_cancel()}</Button>
              </ModalClose>
              <ModalClose asChild>
                <Button variant="flat" asChild>
                  <Link
                    className="flex items-center justify-center"
                    to={
                      serviceModeEnabled
                        ? '/main/settings/clash/network'
                        : '/main/settings/system/service'
                    }
                  >
                    {serviceModeEnabled
                      ? m.settings_label_network()
                      : m.settings_label_service()}
                  </Link>
                </Button>
              </ModalClose>
            </CardFooter>
          </Card>
        </ModalContent>
      </Modal>
    </div>
  );
}
