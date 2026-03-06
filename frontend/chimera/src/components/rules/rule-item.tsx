import type { ClashRule } from '@chimera/interface';
import { Box, type SxProps, type Theme } from '@mui/material';

interface Props {
  index: number;
  value: ClashRule;
  searchText?: string;
}

const COLOR = [
  (theme: Theme) => ({
    color: theme.vars.palette.primary.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.secondary.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.info.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.warning.main,
  }),
  (theme: Theme) => ({
    color: theme.vars.palette.success.main,
  }),
] satisfies SxProps<Theme>[];

const RuleItem = ({ index, value, searchText }: Props) => {
  const parseColorSx = (text: string): SxProps<Theme> => {
    const typeMap = {
      reject: ['REJECT', 'REJECT-DROP'],
      direct: ['DIRECT'],
    };

    if (typeMap.reject.includes(text)) {
      return (theme) => ({ color: theme.vars.palette.error.main });
    }

    if (typeMap.direct.includes(text)) {
      return (theme) => ({ color: theme.vars.palette.text.primary });
    }

    let sum = 0;
    for (let i = 0; i < text.length; i++) {
      sum += text.charCodeAt(i);
    }

    return COLOR[sum % COLOR.length];
  };

  const renderHighlightedText = (text: string) => {
    const keyword = searchText?.trim();
    if (!keyword) {
      return text;
    }

    const pattern = new RegExp(
      `(${keyword.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')})`,
      'ig',
    );
    const parts = text.split(pattern);

    return parts.map((part, index) => {
      if (part.toLowerCase() === keyword.toLowerCase()) {
        return (
          <mark
            key={`${part}-${index}`}
            className="rounded-sm bg-amber-200/70 px-0.5 text-inherit dark:bg-amber-500/30"
          >
            {part}
          </mark>
        );
      }

      return part;
    });
  };

  return (
    <div className="grid grid-cols-[5rem_10rem_minmax(0,1fr)_10rem] items-start gap-4 border-b border-black/5 px-8 py-3 select-text last:border-b-0 dark:border-white/5">
      <Box
        sx={(theme) => ({ color: theme.vars.palette.text.secondary })}
        className="text-sm tabular-nums"
      >
        {index + 1}
      </Box>

      <div className="min-w-0 text-sm font-medium">
        {renderHighlightedText(value.type || '-')}
      </div>

      <Box
        sx={(theme) => ({ color: theme.vars.palette.text.primary })}
        className="min-w-0 break-all"
      >
        {renderHighlightedText(value.payload || '-')}
      </Box>

      <Box
        className="min-w-0 text-sm font-medium break-all"
        sx={parseColorSx(value.proxy)}
      >
        {renderHighlightedText(value.proxy)}
      </Box>
    </div>
  );
};

export default RuleItem;
