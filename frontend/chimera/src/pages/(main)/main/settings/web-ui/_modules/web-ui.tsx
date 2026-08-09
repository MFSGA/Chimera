import { openThat, useClashInfo, useSetting } from '@chimera/interface';
import AddRounded from '~icons/material-symbols/add-rounded';
import AllInboxRounded from '~icons/material-symbols/all-inbox-outline-rounded';
import DeleteRounded from '~icons/material-symbols/delete-rounded';
import EditSquareRounded from '~icons/material-symbols/edit-square-rounded';
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useEffect, useMemo, useState, type PropsWithChildren } from 'react';
import { Button } from '@/components/ui/button';
import {
  Card,
  CardContent,
  CardFooter,
  CardHeader,
} from '@/components/ui/card';
import { Input } from '@/components/ui/input';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
} from '@/components/ui/modal';
import TextMarquee from '@/components/ui/text-marquee';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  SettingsCard,
  SettingsCardContent,
} from '../../_modules/settings-card';

type UrlLabels = {
  host: string;
  port: number;
  secret?: string;
};

const useUrlLabels = (): UrlLabels => {
  const { data } = useClashInfo();

  return useMemo(() => {
    const [host = '127.0.0.1', port = '7890'] = data?.server?.split(':') ?? [];

    return {
      host,
      port: Number(port) || 7890,
      secret: data?.secret ?? undefined,
    };
  }, [data]);
};

const formatUrl = (url: string, labels: UrlLabels) => {
  return Object.entries(labels).reduce((result, [key, value]) => {
    return result.replace(new RegExp(`%${key}`, 'g'), String(value ?? ''));
  }, url);
};

const PreviewItem = ({ url }: { url: string }) => {
  const labels = useUrlLabels();
  const formattedUrl = formatUrl(url, labels);

  return (
    <motion.div
      className="outline-outline-variant overflow-hidden rounded-2xl p-3 outline"
      initial={{ height: 0, opacity: 0 }}
      animate={{ height: 'auto', opacity: 1 }}
      exit={{ height: 0, opacity: 0 }}
    >
      <div>{m.settings_web_ui_preview_title()}</div>
      <TextMarquee className="w-full">{formattedUrl}</TextMarquee>
    </motion.div>
  );
};

const EditItemButton = ({
  defaultUrl,
  index,
  children,
}: PropsWithChildren<{ defaultUrl?: string; index?: number }>) => {
  const [open, setOpen] = useState(false);
  const { value, upsert } = useSetting('web_ui_list');
  const labels = useUrlLabels();
  const [draft, setDraft] = useState(defaultUrl ?? '');

  useEffect(() => setDraft(defaultUrl ?? ''), [defaultUrl]);

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) setDraft(defaultUrl ?? '');
    setOpen(nextOpen);
  };

  const handleSubmit = async () => {
    const nextUrl = draft.trim();
    if (!nextUrl) return;

    try {
      const list = [...(value || [])];
      if (index == null) list.push(nextUrl);
      else list[index] = nextUrl;
      await upsert(list);
      handleOpenChange(false);
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    }
  };

  return (
    <Modal open={open} onOpenChange={handleOpenChange}>
      <ModalTrigger asChild>{children}</ModalTrigger>
      <ModalContent>
        <Card className="w-96">
          <CardHeader>
            <ModalTitle>
              {index == null
                ? m.settings_web_ui_new_item_title()
                : m.settings_web_ui_edit_item_title()}
            </ModalTitle>
          </CardHeader>
          <CardContent>
            <Input
              variant="outlined"
              label={m.settings_web_ui_input_label()}
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
            />

            <p className="flex flex-wrap items-center gap-1 text-sm select-text">
              <span>{m.settings_web_ui_replace_with_label()}</span>
              {Object.keys(labels).map((key) => (
                <span
                  key={key}
                  className="bg-on-primary rounded-full px-2 py-0.5"
                >
                  %{key}
                </span>
              ))}
            </p>

            <AnimatePresence>
              {draft && <PreviewItem url={draft} />}
            </AnimatePresence>
          </CardContent>
          <CardFooter className="gap-2">
            <Button
              variant="flat"
              disabled={!draft.trim()}
              onClick={() => void handleSubmit()}
            >
              {m.common_submit()}
            </Button>
            <ModalClose>{m.common_cancel()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </Modal>
  );
};

const WebUIItem = ({ url, index }: { url: string; index: number }) => {
  const labels = useUrlLabels();
  const formattedUrl = formatUrl(url, labels);
  const { value, upsert } = useSetting('web_ui_list');

  return (
    <Card className="w-full min-w-0 space-y-4 overflow-hidden">
      <CardHeader className="flex w-full min-w-0 flex-row">
        <TextMarquee className="relative w-0 min-w-0 flex-1 text-base">
          {formattedUrl}
        </TextMarquee>
      </CardHeader>
      <CardFooter className="gap-1">
        <Button icon variant="flat" onClick={() => void openThat(formattedUrl)}>
          <OpenInNewRounded className="size-5" />
        </Button>
        <EditItemButton defaultUrl={url} index={index}>
          <Button icon>
            <EditSquareRounded className="size-5" />
          </Button>
        </EditItemButton>
        <Button
          icon
          onClick={() =>
            void upsert(
              (value || []).filter((_, itemIndex) => itemIndex !== index),
            )
          }
        >
          <DeleteRounded className="size-5" />
        </Button>
      </CardFooter>
    </Card>
  );
};

const EmptyItem = () => (
  <Card variant="outline">
    <CardContent className="min-h-40 items-center justify-center">
      <AllInboxRounded className="size-10" />
      <p>{m.settings_web_ui_empty_item()}</p>
    </CardContent>
  </Card>
);

export default function WebUI() {
  const { value } = useSetting('web_ui_list');

  return (
    <div className="space-y-3">
      <SettingsCard data-slot="web-ui-card">
        <SettingsCardContent>
          {value?.length ? (
            value.map((item, index) => (
              <WebUIItem key={`${item}-${index}`} url={item} index={index} />
            ))
          ) : (
            <EmptyItem />
          )}
        </SettingsCardContent>
      </SettingsCard>

      <div className="flex justify-end">
        <EditItemButton>
          <Button className="flex items-center gap-1 px-4" variant="raised">
            <AddRounded className="size-6" />
            <span>{m.settings_web_ui_add_button()}</span>
          </Button>
        </EditItemButton>
      </div>
    </div>
  );
}
