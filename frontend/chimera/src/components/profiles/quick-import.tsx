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
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Notice } from '@/components/base';
import { formatError } from '@/utils';

export interface QuickImportProps {
  defaultUrl?: string | null;
  onImported?: () => void;
}

export const QuickImport = ({ defaultUrl, onImported }: QuickImportProps) => {
  const { t } = useTranslation();

  const [url, setUrl] = useState('');

  const [loading, setLoading] = useState(false);

  const { create } = useProfile();

  useEffect(() => {
    setUrl(defaultUrl ?? '');
  }, [defaultUrl]);

  const onCopyLink = async () => {
    const text = await navigator.clipboard.readText().catch(() => '');

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
          <Tooltip title={t('Clear')}>
            <IconButton size="small" onClick={() => setUrl('')}>
              <ClearRounded fontSize="inherit" />
            </IconButton>
          </Tooltip>

          <Tooltip title={t('Download')}>
            <IconButton size="small" onClick={handleImport}>
              <Download fontSize="inherit" />
            </IconButton>
          </Tooltip>
        </>
      );
    }

    return (
      <Tooltip title={t('Paste')}>
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
      Notice.success(t('Successfully imported profile'));
      setUrl('');
      onImported?.();
    } catch (error) {
      Notice.error(
        t('Failed to import profile: {{error}}', {
          error: formatError(error),
        }),
        3000,
      );
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
      placeholder={t('Profile URL')}
      onChange={(e) => setUrl(e.target.value)}
      onKeyDown={(e) => url !== '' && e.key === 'Enter' && handleImport()}
      sx={{ input: { py: 1, px: 2 } }}
      slotProps={{
        input: inputProps,
      }}
    />
  );
};
