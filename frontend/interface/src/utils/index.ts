type Result<T, E> = { status: 'ok'; data: T } | { status: 'error'; error: E };

export function unwrapResult<T, E>(res: Result<T, E>): T {
  if (res.status === 'error') {
    throw res.error;
  }

  return res.data;
}

// export * from './get-system'
// export * from './retry'
