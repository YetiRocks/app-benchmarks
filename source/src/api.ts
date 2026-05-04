// Vite-injected defines (see vite.config.ts):
//   __STATIC_ROOT__    = '' for root-mounted apps, '/<app-id>' otherwise
//   __RESOURCES_ROOT__ = the resources.route from yeti-config.yaml (e.g. 'api')
// For app-benchmarks this resolves to '/app-benchmarks/api'.
const BASE = `${__STATIC_ROOT__}/${__RESOURCES_ROOT__}`;

export async function api(path: string, options?: RequestInit): Promise<Response> {
  const url = `${BASE}${path}`;
  const res = await fetch(url, options);
  if (!res.ok) {
    const text = await res.text().catch(() => res.statusText);
    throw new Error(`${res.status}: ${text}`);
  }
  return res;
}
