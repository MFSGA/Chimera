import { openThat } from '@chimera/interface';
import { alpha } from '@chimera/ui';
import DeleteRounded from '@mui/icons-material/DeleteRounded';
import EditRounded from '@mui/icons-material/EditRounded';
import OpenInNewRounded from '@mui/icons-material/OpenInNewRounded';
import Box from '@mui/material/Box';
import Chip from '@mui/material/Chip';
import IconButton from '@mui/material/IconButton';
import Paper, { PaperProps } from '@mui/material/Paper';
import { styled } from '@mui/material/styles';
import Typography from '@mui/material/Typography';
import { ReactElement, ReactNode } from 'react';
import ReactFastMarquee from 'react-fast-marquee';

const Marquee = ((
  ReactFastMarquee as unknown as { default?: typeof ReactFastMarquee }
).default ?? ReactFastMarquee) as typeof ReactFastMarquee;

type WebUrlLabels = {
  [label: string]: string | number | undefined | null;
};

export const renderChip = (
  string: string,
  labels: WebUrlLabels,
): (string | ReactElement)[] => {
  return string.split(/(%[^&?]+)/).map((part, index) => {
    if (part.startsWith('%')) {
      const label = labels[part.replace('%', '')];

      if (!label) {
        return '';
      }

      return (
        <Chip
          sx={{
            '& .MuiChip-label': {
              pl: 0.5,
              pr: 0.5,
            },
          }}
          key={index}
          size="small"
          label={label}
        />
      );
    }

    return part;
  });
};

export const extractServer = (
  string?: string,
): { host: string; port: number } => {
  if (!string) {
    return { host: '127.0.0.1', port: 7890 };
  }

  const [host, port] = string.split(':');

  return { host, port: Number(port) };
};

export const openWebUrl = (string: string, labels: WebUrlLabels): void => {
  let url = string;

  for (const key in labels) {
    const label = labels[key];

    if (label == null) {
      continue;
    }

    const regex = new RegExp(`%${key}`, 'g');

    url = url.replace(regex, String(label));
  }

  openThat(url);
};

export const Item = styled(Paper)<PaperProps>(({ theme }) => ({
  backgroundColor: alpha(theme.vars.palette.primary.main, 0.1),
  padding: 16,
  borderRadius: 16,
  display: 'flex',
  flexDirection: 'column',
  gap: 8,
})) as typeof Paper;

export interface ClashWebItemProps {
  label: ReactNode;
  onOpen: () => void;
  onDelete: () => void;
  onEdit: () => void;
}

export const ClashWebItem = ({
  label,
  onOpen,
  onDelete,
  onEdit,
}: ClashWebItemProps) => {
  return (
    <Item>
      <Marquee>
        <Typography variant="subtitle1" sx={{ marginRight: 16 }}>
          {label}
        </Typography>
      </Marquee>

      <Box
        sx={{
          display: 'flex',
          justifyContent: 'end',
          alignItems: 'center',
          gap: 1,
        }}
      >
        <IconButton onClick={onOpen}>
          <OpenInNewRounded />
        </IconButton>

        <IconButton onClick={onEdit}>
          <EditRounded />
        </IconButton>

        <IconButton onClick={onDelete}>
          <DeleteRounded />
        </IconButton>
      </Box>
    </Item>
  );
};
