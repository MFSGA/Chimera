import { useClashInfo, useSetting } from '@chimera/interface';
import { BaseCard, BaseDialog, Expand } from '@chimera/ui';
import AddIcon from '@mui/icons-material/Add';
import {
  Box,
  Chip,
  Divider,
  IconButton,
  TextField,
  Tooltip,
  Typography,
} from '@mui/material';
import Grid from '@mui/material/Grid';
import { useLockFn } from 'ahooks';
import { useMemo, useState } from 'react';
import * as m from '@/paraglide/messages';
import {
  ClashWebItem,
  extractServer,
  openWebUrl,
  renderChip,
} from './modules/clash-web';

const AddRecordButton = ({ onClick }: { onClick: () => void }) => {
  return (
    <Tooltip title={m.settings_web_ui_new_item_title()}>
      <IconButton onClick={onClick}>
        <AddIcon />
      </IconButton>
    </Tooltip>
  );
};

export const SettingClashWeb = () => {
  const { value, upsert } = useSetting('web_ui_list');

  const { data } = useClashInfo();

  const labels = useMemo(() => {
    const { host, port } = extractServer(data?.server);

    return {
      host,
      port,
      secret: data?.secret,
    };
  }, [data]);

  const [open, setOpen] = useState(false);

  const [editString, setEditString] = useState('');

  const [editIndex, setEditIndex] = useState<number | null>(null);

  const deleteItem = useLockFn(async (index: number) => {
    await upsert(
      value ? value.slice(0, index).concat(value.slice(index + 1)) : null,
    );
  });

  const updateItem = useLockFn(async () => {
    const list = [...(value || [])];

    if (editIndex !== null) {
      list[editIndex] = editString;
    } else {
      list.push(editString);
    }

    await upsert(list);
  });

  return (
    <>
      <BaseCard
        label={m.settings_web_ui_title()}
        labelChildren={
          <AddRecordButton
            onClick={() => {
              setEditString('');
              setEditIndex(null);
              setOpen(true);
            }}
          />
        }
      >
        {value && (
          <Grid container sx={{ mt: 1 }} spacing={2}>
            {value.map((item, index) => {
              return (
                <Grid
                  key={index}
                  size={{
                    xs: 12,
                    xl: 6,
                  }}
                >
                  <ClashWebItem
                    label={renderChip(item, labels)}
                    onOpen={() => openWebUrl(item, labels)}
                    onEdit={() => {
                      setEditIndex(index);
                      setEditString(item);
                      setOpen(true);
                    }}
                    onDelete={() => deleteItem(index)}
                  />
                </Grid>
              );
            })}
          </Grid>
        )}
      </BaseCard>

      <BaseDialog
        title={
          editIndex != null
            ? m.settings_web_ui_edit_item_title()
            : m.settings_web_ui_new_item_title()
        }
        open={open}
        onClose={() => {
          setOpen(false);
          setEditIndex(null);
        }}
        onOk={() => {
          updateItem();
          setOpen(false);
          setEditIndex(null);
          setEditString('');
        }}
        ok={m.common_ok()}
        close={m.common_close()}
        contentStyle={{ overflow: editString ? 'auto' : 'hidden' }}
        divider
      >
        <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
          <Typography variant="h5">
            {m.settings_web_ui_input_title()}
          </Typography>

          <TextField
            sx={{ width: '100%' }}
            value={editString}
            variant="outlined"
            multiline
            placeholder={m.settings_web_ui_replace_support_hint()}
            onChange={(e) => setEditString(e.target.value)}
          />

          <Typography sx={{ userSelect: 'text' }}>
            {m.settings_web_ui_replace_with_label()}
          </Typography>

          <Box sx={{ display: 'flex', gap: 1 }}>
            {Object.entries(labels).map(([key]) => {
              return <Chip key={key} size="small" label={`%${key}`} />;
            })}
          </Box>

          <Expand open={Boolean(editString)}>
            <Box sx={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
              <Divider sx={{ mt: 1, mb: 1 }} />

              <Typography variant="h5">
                {m.settings_web_ui_preview_title()}
              </Typography>

              <Typography sx={{ userSelect: 'text' }} component="div">
                {renderChip(editString, labels)}
              </Typography>
            </Box>
          </Expand>
        </Box>
      </BaseDialog>
    </>
  );
};

export default SettingClashWeb;
