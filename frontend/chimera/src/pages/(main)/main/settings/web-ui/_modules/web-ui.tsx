import {
  openThat,
  useClashInfo,
  useClashSetting,
  useSetting,
} from '@chimera/interface';
import { Button } from '@chimera/ui/button';
import { Card, CardContent, CardFooter, CardHeader } from '@chimera/ui/card';
import { Input } from '@chimera/ui/input';
import {
  Modal,
  ModalClose,
  ModalContent,
  ModalTitle,
  ModalTrigger,
  useModal,
} from '@chimera/ui/modal';
import TextMarquee from '@chimera/ui/text-marquee';
import AddRounded from '~icons/material-symbols/add-rounded';
import DeleteRounded from '~icons/material-symbols/delete-rounded';
import EditSquareRounded from '~icons/material-symbols/edit-square-rounded';
import OpenInNewRounded from '~icons/material-symbols/open-in-new-rounded';
import { AnimatePresence, motion } from 'motion/react';
import { useMemo, useState, type PropsWithChildren } from 'react';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';
import { message } from '@/utils/notification';
import {
  SettingsList,
  SettingsListActions,
  SettingsListContent,
  SettingsListItem,
} from '../../_modules/settings-list';

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

const EditItemButton = (
  props: PropsWithChildren<{ defaultUrl?: string; index?: number }>,
) => (
  <Modal>
    <EditItemForm {...props} />
  </Modal>
);

const EditItemForm = ({
  defaultUrl,
  index,
  children,
}: PropsWithChildren<{ defaultUrl?: string; index?: number }>) => {
  const { setOpen } = useModal();
  const [saving, setSaving] = useState(false);
  const { value, upsert } = useClashSetting('web_ui_list');
  const labels = useUrlLabels();
  const [draft, setDraft] = useState(defaultUrl ?? '');

  const handleSubmit = async () => {
    const nextUrl = draft.trim();
    if (!nextUrl) return;

    if (saving) return;
    setSaving(true);
    try {
      const list = [...(value || [])];
      if (index == null) list.push(nextUrl);
      else list[index] = nextUrl;
      await upsert(list);
      setOpen(false);
    } catch (error) {
      message(formatError(error), {
        title: m.common_error(),
        kind: 'error',
      });
    } finally {
      setSaving(false);
    }
  };

  return (
    <>
      <ModalTrigger asChild>{children}</ModalTrigger>
      <ModalContent
        dismissible={!saving}
        onOpenAutoFocus={() => setDraft(defaultUrl ?? '')}
      >
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
              disabled={!draft.trim() || saving}
              loading={saving}
              onClick={() => void handleSubmit()}
            >
              {m.common_submit()}
            </Button>
            <ModalClose disabled={saving}>{m.common_cancel()}</ModalClose>
          </CardFooter>
        </Card>
      </ModalContent>
    </>
  );
};

const WebUIItem = ({ url, index }: { url: string; index: number }) => {
  const labels = useUrlLabels();
  const formattedUrl = formatUrl(url, labels);
  const { value, upsert } = useClashSetting('web_ui_list');

  return (
    <SettingsListItem data-slot="web-ui-item">
      <TextMarquee className="relative w-0 min-w-0 flex-1 text-base">
        {formattedUrl}
      </TextMarquee>

      <div
        className="flex shrink-0 items-center gap-1"
        data-slot="web-ui-item-actions"
      >
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
      </div>
    </SettingsListItem>
  );
};

export default function WebUI() {
  const { value } = useClashSetting('web_ui_list');

  return (
    <SettingsList data-slot="web-ui-card">
      <SettingsListContent
        data-slot="web-ui-card-content"
        empty={!value?.length}
        emptyMessage={m.settings_web_ui_empty_item()}
      >
        {value?.map((item, index) => (
          <WebUIItem key={`${item}-${index}`} url={item} index={index} />
        ))}
      </SettingsListContent>

      <SettingsListActions>
        <EditItemButton>
          <Button className="flex items-center gap-1 px-4" variant="raised">
            <AddRounded className="size-6" />
            <span>{m.settings_web_ui_add_button()}</span>
          </Button>
        </EditItemButton>
      </SettingsListActions>
    </SettingsList>
  );
}
