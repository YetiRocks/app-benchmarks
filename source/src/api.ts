const BASE = '/api';

export async function api(path: string, options?: RequestInit): Promise<Response> {
  const url = `${BASE}${path}`;
  const res = await fetch(url, options);
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`${res.status}: ${text}`);
  }
  return res;
}
