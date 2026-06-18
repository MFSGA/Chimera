import { useProfile } from '@chimera/interface';
import { alpha } from '@chimera/ui';
import {
  ClearRounded,
  ContentCopyRounded,
  Download,
} from '@mui/icons-material';
import {
  CircularProgress,
  FilledInputProps,
  IconButton,
  TextField,
  Tooltip,
} from '@mui/material';
import { readText } from '@tauri-apps/plugin-clipboard-manager';
import { useState } from 'react';
import { Notice } from '@/components/base';
import * as m from '@/paraglide/messages';
import { formatError } from '@/utils';

export const QuickImport = () => {
  const [url, setUrl] = useState('');

  const [loading, setLoading] = useState(false);

  const { create } = useProfile();

  const onCopyLink = async () => {
    const text = await readText().catch(() => '');

    if (text) {
      setUrl(text);
    }
  };

  const endAdornment = () => {
    if (loading) {
      return <CircularProgress size={20} />;
    }

    if (url) {
      return (
        <>
          <Tooltip title={m.common_clear()}>
            <IconButton size="small" onClick={() => setUrl('')}>
              <ClearRounded fontSize="inherit" />
            </IconButton>
          </Tooltip>

          <Tooltip title={m.profile_subscription_update()}>
            <IconButton size="small" onClick={handleImport}>
              <Download fontSize="inherit" />
            </IconButton>
          </Tooltip>
        </>
      );
    }

    return (
      <Tooltip title={m.common_paste()}>
        <IconButton size="small" onClick={onCopyLink}>
          <ContentCopyRounded fontSize="inherit" />
        </IconButton>
      </Tooltip>
    );
  };

  const handleImport = async () => {
    try {
      setLoading(true);

      await create.mutateAsync({
        type: 'url',
        data: {
          url,
          option: null,
        },
      });
      Notice.success(m.profile_quick_import_success_message());
      setUrl('');
    } catch (error) {
      Notice.error(`Failed to import profile: ${formatError(error)}`, 3000);
    } finally {
      setLoading(false);
    }
  };

  const inputProps: Partial<FilledInputProps> = {
    sx: (theme) => ({
      borderRadius: 7,
      backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
      fieldset: {
        border: 'none',
      },
    }),
    endAdornment: endAdornment(),
  };

  return (
    <TextField
      hiddenLabel
      fullWidth
      autoComplete="off"
      spellCheck="false"
      value={url}
      placeholder={m.profile_import_remote_url_label()}
      onChange={(e) => setUrl(e.target.value)}
      onKeyDown={(e) => url !== '' && e.key === 'Enter' && handleImport()}
      sx={{ input: { py: 1, px: 2 } }}
      slotProps={{
        input: inputProps,
      }}
    />
  );
};
