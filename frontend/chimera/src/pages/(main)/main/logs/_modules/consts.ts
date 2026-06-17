/**
 * 日志页面常量
 *
 * 迁移自 ref: `src/pages/(main)/main/logs/_modules/consts.ts`
 *
 * 定义日志级别的枚举，包含所有 Clash 核心支持的日志类型：
 * - debug: 调试信息（最详细）
 * - info: 普通信息
 * - warning: 警告信息
 * - error: 错误信息
 */
export enum LogLevel {
  Debug = 'debug',
  Info = 'info',
  Warning = 'warning',
  Error = 'error',
}
