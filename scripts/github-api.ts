const GITHUB_API_HEADERS = {
  Accept: 'application/vnd.github+json',
  'User-Agent': 'clash-nyanpasu',
};

export function githubApiHeaders(token?: string): Headers {
  const headers = new Headers(GITHUB_API_HEADERS);
  if (token) headers.set('Authorization', `Bearer ${token}`);
  return headers;
}
