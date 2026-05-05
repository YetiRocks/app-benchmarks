import { useEffect, useState } from 'react'

/**
 * Gates the app on yeti-auth login state. Returns:
 *   - `null`  while the auth check is in flight
 *   - `true`  if the app has no auth providers configured (open access),
 *             or if there's a valid OAuth user session
 *   - `false` if auth is configured but the user isn't signed in
 */
export function useAuth(): boolean | null {
  const [authenticated, setAuthenticated] = useState<boolean | null>(null)

  useEffect(() => {
    fetch('/yeti-auth/oauth_providers?app_id=app-benchmarks')
      .then(r => r.ok ? r.json() : null)
      .then(data => {
        if (!data?.providers?.length) {
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
