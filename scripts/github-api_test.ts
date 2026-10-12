import { githubApiHeaders } from './github-api.ts';

Deno.test('GitHub API requests use the configured bearer token', () => {
  const headers = githubApiHeaders('test-token');
  if (headers.get('Authorization') !== 'Bearer test-token') {
    throw new Error('GitHub API authorization header was not set.');
  }
  if (headers.get('Accept') !== 'application/vnd.github+json') {
    throw new Error('GitHub API Accept header was not preserved.');
  }
});

Deno.test('public GitHub requests omit an empty authorization header', () => {
  const headers = githubApiHeaders();
  if (headers.has('Authorization')) {
    throw new Error('A missing token must not create an authorization header.');
  }
});
