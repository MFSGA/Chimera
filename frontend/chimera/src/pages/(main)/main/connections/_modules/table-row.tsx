/**
 * 连接表格行组件
 *
 * 迁移自 ref: `src/pages/(main)/main/connections/_modules/table-row.tsx`
 *
 * 职责：
 * - 渲染连接表格的每一行
 * - 支持双击查看连接详情（Modal）
 * - 提供右键上下文菜单（查看详情 / 关闭连接）
 *
 * 当前阶段（Connections Step 2 - 迁移至 ref 实现）：
 * - 使用 @tanstack/react-table + react-virtual 替代 MRT
 * - 添加连接详情对话框（双击/右键菜单触发）
 * - 添加右键关闭连接功能
 *
 * 后续：
 * - 连接详情中可添加更多可视化信息（流量折线图等）
 */

import { useClashConnections } from '@chimera/interface';
import { BaseDialog, cn } from '@chimera/ui';
import { Close, InfoOutlined } from '@mui/icons-material';
import { Button, List, ListItem, Menu, MenuItem } from '@mui/material';
import { useLockFn } from 'ahooks';
import { sentenceCase } from 'change-case';
import dayjs from 'dayjs';
import { filesize } from 'filesize';
import {
  ComponentProps,
  useCallback,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import * as m from '@/paraglide/messages';
import type { ConnectionRow } from '../index';

/**
 * 连接详情对话框 - 在双击行或右键菜单「查看详情」时打开
 * 使用 MUI BaseDialog 实现，与 Chimera 现有对话框模式一致
 */
function ConnectionDetailDialog({
  data,
  open,
  onClose,
}: {
  data: ConnectionRow;
  open: boolean;
  onClose: () => void;
}) {
  const { deleteConnections } = useClashConnections();

  /**
   * 关闭当前连接
   * 先关闭对话框再执行删除，避免删除缓慢时显示过期数据
   */
  const handleCloseConnection = useLockFn(async () => {
    onClose();
    await deleteConnections.mutateAsync(data.id);
  });

  return (
    <BaseDialog
      title={m.connections_view_details()}
      open={open}
      onClose={onClose}
      divider
      contentStyle={{ minWidth: 640 }}
    >
      <List dense disablePadding>
        {Object.entries(data)
          .filter(
            ([key, value]) =>
              key !== 'metadata' &&
              !(['closed', 'downloadSpeed', 'uploadSpeed'] as const).includes(
                key as never,
              ) &&
              value !== undefined &&
              value !== null &&
              value !== '',
          )
          .map(([key, value]) => (
            <DetailListItem key={key} label={key} value={value} />
          ))}

        <ListItem disableGutters>
          <div className="pt-2 text-sm font-semibold">
            {m.connections_metadata_label()}
          </div>
        </ListItem>

        {Object.entries(data.metadata)
          .filter(
            ([, value]) =>
              value !== undefined && value !== null && value !== '',
          )
          .map(([key, value]) => (
            <DetailListItem key={key} label={key} value={value} />
          ))}
      </List>

      <div className="mt-4 flex justify-end gap-2">
        <Button variant="outlined" onClick={onClose} color="inherit">
          {m.common_close()}
        </Button>
        <Button
          variant="contained"
          color="error"
          onClick={handleCloseConnection}
        >
          {m.connections_close_connection()}
        </Button>
      </div>
    </BaseDialog>
  );
}

function DetailListItem({ label, value }: { label: string; value: unknown }) {
  const mono =
    label === 'id' ||
    label.includes('IP') ||
    label.includes('Port') ||
    label.includes('Destination') ||
    label.includes('Source');

  return (
    <ListItem disableGutters divider>
      <div className="flex min-w-0 flex-col py-1">
        <div className="text-on-surface-variant text-xs">
          {sentenceCase(label)}
        </div>
        <div className={cn('text-sm break-all', mono && 'font-mono text-xs')}>
          {formatDetailValue(label, value)}
        </div>
      </div>
    </ListItem>
  );
}

/**
 * 格式化详情对话框中的值
 * 对 speed 字段使用 filesize，对日期字段使用 dayjs.fromNow
 */
function formatDetailValue(key: string, value: unknown): ReactNode {
  if (Array.isArray(value)) {
    return <span>{value.join(' / ')}</span>;
  }

  const k = key.toLowerCase();

  if (k.includes('speed')) {
    return <span>{filesize(value as number)}/s</span>;
  }

  if (k.includes('download') || k.includes('upload')) {
    return <span>{filesize(value as number)}</span>;
  }

  if (k.includes('port') || k === 'id' || k.includes('ip')) {
    return <span>{String(value)}</span>;
  }

  // 尝试解析日期字符串
  if (
    typeof value === 'string' &&
    value.includes('T') &&
    dayjs(value).isValid()
  ) {
    return (
      <span title={dayjs(value).format('YYYY-MM-DD HH:mm:ss')}>
        {dayjs(value).fromNow()}
      </span>
    );
  }

  return <span>{String(value)}</span>;
}

/**
 * 连接表格行组件
 *
 * 提供：
 * - 双击打开连接详情
 * - 右键上下文菜单（查看详情 / 关闭连接）
 *
 * 使用方式与 ref 一致：
 * 1. 通过 RegisterContextMenu 配合右键菜单
 * 2. 通过 onDoubleClick 打开详情对话框
 */
export default function TableRow({
  data,
  ...props
}: ComponentProps<'tr'> & {
  data: ConnectionRow;
}) {
  const { deleteConnections } = useClashConnections();

  // 详情对话框状态
  const [detailOpen, setDetailOpen] = useState(false);

  // 右键上下文菜单状态
  const [contextMenu, setContextMenu] = useState<{
    mouseX: number;
    mouseY: number;
  } | null>(null);

  /**
   * 处理右键打开上下文菜单
   */
  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      setContextMenu(
        contextMenu === null
          ? { mouseX: e.clientX - 2, mouseY: e.clientY - 4 }
          : null,
      );
    },
    [contextMenu],
  );

  /**
   * 关闭上下文菜单
   */
  const handleCloseContextMenu = useCallback(() => {
    setContextMenu(null);
  }, []);

  /**
   * 关闭当前连接
   */
  const handleCloseConnection = useLockFn(async () => {
    handleCloseContextMenu();
    if (detailOpen) {
      setDetailOpen(false);
    }
    await deleteConnections.mutateAsync(data.id);
  });

  return (
    <>
      {/* 表格行 - 支持双击和右键 */}
      <tr
        onDoubleClick={() => setDetailOpen(true)}
        onContextMenu={handleContextMenu}
        {...props}
      />

      {/* 右键上下文菜单 */}
      <Menu
        open={contextMenu !== null}
        onClose={handleCloseContextMenu}
        anchorReference="anchorPosition"
        anchorPosition={
          contextMenu !== null
            ? { top: contextMenu.mouseY, left: contextMenu.mouseX }
            : undefined
        }
      >
        <MenuItem
          onClick={() => {
            setDetailOpen(true);
            handleCloseContextMenu();
          }}
        >
          <InfoOutlined className="mr-2 !size-4" />
          {m.connections_view_details()}
        </MenuItem>
        <MenuItem onClick={handleCloseConnection}>
          <Close className="mr-2 !size-4" />
          {m.connections_close_connection()}
        </MenuItem>
      </Menu>

      {/* 连接详情对话框 */}
      <ConnectionDetailDialog
        data={data}
        open={detailOpen}
        onClose={() => setDetailOpen(false)}
      />
    </>
  );
}
