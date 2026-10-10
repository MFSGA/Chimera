import dayjs from 'dayjs';
import relativeTime from 'dayjs/plugin/relativeTime';
import 'dayjs/locale/ru';
import 'dayjs/locale/zh-cn';
import 'dayjs/locale/zh-tw';
import type { Locale } from '@/paraglide/runtime';

dayjs.extend(relativeTime);

type DateInput = Date | number | string | null | undefined;

export function formatDate(value: DateInput) {
  if (value == null) return '-';
  const date = dayjs(value);
  return date.isValid() ? date.format('YYYY-MM-DD HH:mm:ss') : 'Invalid Date';
}

export function formatRelativeTime(
  value: DateInput,
  now: Date | number,
  language: Locale,
) {
  if (value == null) return '-';
  const date = dayjs(value);
  return date.isValid() ? date.locale(language).from(now) : 'Invalid Date';
}
