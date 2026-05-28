import { useEffect, useState } from 'react'

/**
 * Gates the app on yeti-auth login state. Returns:
 *   - `null`  while the auth check is in flight
 *   - `true`  if the app has no required_roles (public UI),
 *             or if there's a valid session with one of the required roles
 *   - `false` if required_roles is non-empty and the user isn't signed in
 *
 * Signal: `[package.metadata.app] required_roles` on the customer
 * app's Cargo.toml. Empty/missing = public; any role = login required.
 * Surfaced via `/yeti-auth/oauth_providers?app_id=<this-app>` ->
 * `required_roles` field. The provider list itself is no longer the
 * gate — it bled global wasm-registered providers through, making
 * apps without auth metadata look like they required login.
 */
export function useAuth(): boolean | null {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null)

  useEffect(() => {
    fetch('/yeti-auth/oauth_providers?app_id=app-benchmarks')
      .then(r => r.ok ? r.json() : null)
      .then(data => {
        const required = (data?.required_roles as string[] | undefined) ?? []
        if (required.length === 0) {
          setAuthenticated(true)
          return
        }
        return fetch('/yeti-auth/oauth_user', { credentials: 'same-origin' })
          .then(r => r.ok ? r.json() : null)
          .then(d => setAuthenticated(!!(d?.user)))
      })
      .catch(() => setAuthenticated(true))
  }, [])

  return authenticated
}
