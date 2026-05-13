type ConsoleBridgeLevel =
  | 'console.error'
  | 'console.warn'
  | 'window.error'
  | 'unhandledrejection';

const serialize = (value: unknown): string => {
  if (value instanceof Error) {
    return value.stack || value.message;
  }

  if (typeof value === 'string') {
    return value;
  }

  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
};

export const setupFrontendConsoleBridge = async () => {
  if (!import.meta.env.DEV) {
    return;
  }

  const { emit } = await import('@tauri-apps/api/event');
  const send = (level: ConsoleBridgeLevel, values: unknown[]) => {
    void emit('frontend-console', {
      level,
      message: values.map(serialize).join(' '),
      path: window.location.pathname,
      href: window.location.href,
      timestamp: new Date().toISOString(),
    }).catch(() => {});
  };

  const originalError = console.error.bind(console);
  console.error = (...args) => {
    send('console.error', args);
    originalError(...args);
  };

  const originalWarn = console.warn.bind(console);
  console.warn = (...args) => {
    send('console.warn', args);
    originalWarn(...args);
  };

  window.addEventListener('error', (event) => {
    send('window.error', [
      event.message,
      event.filename,
      event.lineno,
      event.colno,
      event.error,
    ]);
  });

  window.addEventListener('unhandledrejection', (event) => {
    send('unhandledrejection', [event.reason]);
  });
};
