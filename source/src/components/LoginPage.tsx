/**
 * LoginPage — canonical Yeti shared component for app sign-in / sign-up.
 *
 * Source of truth: ~/yeti/shared-components/LoginPage.tsx
 * Synced to each app at: source/src/components/LoginPage.tsx
 * Run ../shared-components/sync.sh to refresh after edits.
 *
 * One unified form, no Sign In / Sign Up tabs:
 *   1. OAuth provider buttons (whichever the platform exposes)
 *   2. Single-line magic-link input + button (form-group style)
 *   3. Optional pre-auth waitlist callout, when `signup.waitlist=true`
 *
 * The same form handles both first-time sign-up and returning sign-in:
 *   - Existing users (waitlist=false): magic link sends a sign-in link;
 *     OAuth lands them at the app authenticated.
 *   - New users on apps with `email_signup=true` or `waitlist=true`: the
 *     magic-link request creates an inactive User row inline (with the
 *     `waitlist` flag set per app policy).
 *
 * Waitlist behavior:
 *   - When the platform sees `waitlist=true` for the app, every new User
 *     gets `waitlist: true`. The platform redirects waitlisted accounts
 *     back to this page with `?waitlisted=true` after authentication.
 *   - This page renders the configured `waitlist_thank_you` message
 *     when `?waitlisted=true` is in the URL.
 *
 * Visible sections are driven by /yeti-auth/oauth_providers — the
 * component fetches `methods`, `providers`, and `signup` for the given
 * appId and shows whichever sections the platform has enabled.
 *
 * redirect_uri uses Vite-injected `__STATIC_ROOT__` so the same source
 * works whether the app is mounted at root or under a tenant prefix.
 *
 * Required CSS classes (provided by ./styles/login.css):
 *   .login-page, .login-card, .login-logo-link, .login-logo,
 *   .login-input-group, .login-input-group input, .login-input-group button,
 *   .login-divider, .login-error, .login-helper, .login-success,
 *   .login-callout, .btn, .btn-oauth, .btn-google, .btn-github,
 *   .btn-primary, .login-submit, .loading
 */

import { type FormEvent, type ReactNode, useEffect, useState } from "react";

const AUTH_BASE = "/yeti-auth";

const DEFAULT_WAITLIST_MESSAGE = "Sign in to be added to our waitlist.";
const DEFAULT_WAITLIST_THANK_YOU =
	"You're on the list. We'll reach out when your account is ready.";

interface SignupConfig {
	/** Whether the platform accepts public signups for this app via the
	 *  passwordless magic-link flow. */
	email_signup: boolean;
	/** Reserved for future use. Kept on the wire for forward compatibility. */
	verification_required: boolean;
	/** Whether this app collects waitlist signups. When true, every new
	 *  User created through magic-link request gets `waitlist: true`. */
	waitlist: boolean;
	/** Pre-auth callout shown beneath the auth controls. */
	waitlist_message: string | null;
	/** Post-auth thank-you shown when a waitlisted user lands back here. */
	waitlist_thank_you: string | null;
}

/** Extra input rendered alongside the email field. The `name` becomes
 *  the key inside the `metadata` object sent to the platform. */
export interface SignupField {
	/** Field key (also used as the metadata object key). */
	name: string;
	/** Visible label / placeholder. */
	label: string;
	/** HTML input type. Defaults to "text". */
	type?: "text" | "email" | "url" | "tel";
	/** When true, form blocks submission until a non-empty value is given. */
	required?: boolean;
}

/** @deprecated — alias for {@link SignupField}. Use SignupField going forward. */
export type WaitlistField = SignupField;

interface LoginPageProps {
	/** Called after a successful sign-in. Magic-link flows do NOT trigger
	 *  this directly — the user lands back at the app via the consume
	 *  redirect, where the app's own auth check picks up the session. */
	onLogin: () => void;
	/** App id passed to /yeti-auth/oauth_providers and /oauth_login. */
	appId: string;
	/** Logo image URL. Defaults to `${BASE_URL}logo_color.svg`. */
	logoSrc?: string;
	/** Logo alt text (app brand name, e.g. "AgentDaddy"). */
	logoAlt: string;
	/** Override the platform-reported `email_signup` flag. */
	signupEnabled?: boolean;
	/** Override the platform-reported `waitlist` flag. */
	waitlistEnabled?: boolean;
	/** Extra fields collected during sign-up / waitlist. Each becomes a
	 *  key in the `metadata` blob sent to the platform. */
	signupFields?: SignupField[];
	/** @deprecated — use `signupFields`. Kept for one-release migration. */
	waitlistFields?: SignupField[];
	/** Override the platform-reported pre-auth waitlist callout. */
	waitlistMessage?: ReactNode;
	/** Override the platform-reported post-auth thank-you. */
	waitlistThankYou?: ReactNode;
	/** Replaces the default "Check your email" message after a successful
	 *  magic-link request. */
	magicLinkSentMessage?: ReactNode;
	/** Helper copy shown beneath the auth form. */
	helperText?: ReactNode;
	/** ── Deprecated props (kept so existing call sites compile) ── */
	/** @deprecated — single unified form has no mode distinction. */
	mode?: "login" | "signup";
	/** @deprecated — tabs have been removed in favor of a single form. */
	signInPath?: string;
	/** @deprecated — tabs have been removed in favor of a single form. */
	signUpPath?: string;
	/** @deprecated — kept for backward compat; no longer affects the UI. */
	allowSignup?: boolean;
}

export default function LoginPage({
	onLogin: _onLogin,
	appId,
	logoSrc,
	logoAlt,
	signupEnabled,
	waitlistEnabled,
	signupFields,
	waitlistFields,
	waitlistMessage,
	waitlistThankYou,
	magicLinkSentMessage,
	helperText,
}: LoginPageProps) {
	const [methods, setMethods] = useState<string[] | null>(null);
	const [providers, setProviders] = useState<string[]>([]);
	const [signupConfig, setSignupConfig] = useState<SignupConfig>({
		email_signup: false,
		verification_required: true,
		waitlist: false,
		waitlist_message: null,
		waitlist_thank_you: null,
	});
	const [email, setEmail] = useState("");
	const [meta, setMeta] = useState<Record<string, string>>({});
	const [error, setError] = useState("");
	const [bannerError, setBannerError] = useState("");
	const [submitting, setSubmitting] = useState(false);
	const [magicLinkSent, setMagicLinkSent] = useState(false);
	const [waitlistedView, setWaitlistedView] = useState(false);

	// signupFields wins; fall back to waitlistFields for one-release migration.
	const fields: SignupField[] = signupFields ?? waitlistFields ?? [];

	useEffect(() => {
		fetch(`${AUTH_BASE}/oauth_providers?app_id=${appId}`, {
			credentials: "same-origin",
		})
			.then((r) => (r.ok ? r.json() : { providers: [], methods: [] }))
			.then((data) => {
				setProviders(
					(data.providers || []).map((p: { name: string }) => p.name),
				);
				setMethods(data.methods || []);
				if (data.signup) {
					setSignupConfig({
						email_signup: !!data.signup.email_signup,
						verification_required: !!data.signup.verification_required,
						waitlist: !!data.signup.waitlist,
						waitlist_message: data.signup.waitlist_message ?? null,
						waitlist_thank_you: data.signup.waitlist_thank_you ?? null,
					});
				}
			})
			.catch(() => {
				setProviders([]);
				setMethods([]);
			});

		// Surface ?error=... and ?waitlisted=true from auth-flow redirects.
		const params = new URLSearchParams(window.location.search);
		const urlError = params.get("error");
		if (urlError) {
			setBannerError(urlError);
		}
		if (params.get("waitlisted") === "true") {
			setWaitlistedView(true);
		}
		if (urlError || params.has("waitlisted")) {
			window.history.replaceState({}, "", window.location.pathname);
		}
	}, [appId]);

	const handleOAuthLogin = (provider: string) => {
		setError("");
		window.location.href = `${AUTH_BASE}/oauth_login?provider=${provider}&redirect_uri=${__STATIC_ROOT__}/app/&app_id=${appId}`;
	};

	const handleMagicLink = async (e: FormEvent) => {
		e.preventDefault();
		setError("");
		// Validate required extras client-side. Server takes whatever it gets.
		for (const f of fields) {
			if (f.required && !meta[f.name]?.trim()) {
				setError(`${f.label} is required`);
				return;
			}
		}
		setSubmitting(true);
		try {
			const res = await fetch(`${AUTH_BASE}/magic-link-request`, {
				method: "POST",
				headers: { "Content-Type": "application/json" },
				credentials: "same-origin",
				body: JSON.stringify({
					email,
					appId,
					signup: true, // unified form: same call serves new + returning
					...(Object.keys(meta).length > 0 ? { metadata: meta } : {}),
				}),
			});
			if (!res.ok) {
				const data = await res.json().catch(() => null);
				setError(data?.error || "Could not send link");
				return;
			}
			setMagicLinkSent(true);
		} catch {
			setError("Connection failed");
		} finally {
			setSubmitting(false);
		}
	};

	if (methods === null) {
		return <div className="loading">Loading...</div>;
	}

	const showOAuth = methods.includes("oauth") && providers.length > 0;
	const resolvedLogo =
		logoSrc ?? `${import.meta.env.BASE_URL}logo_color.svg`;
	const effectiveSignupEnabled =
		signupEnabled !== undefined ? signupEnabled : signupConfig.email_signup;
	const effectiveWaitlistEnabled =
		waitlistEnabled !== undefined ? waitlistEnabled : signupConfig.waitlist;
	const effectiveWaitlistMessage =
		waitlistMessage ??
		signupConfig.waitlist_message ??
		DEFAULT_WAITLIST_MESSAGE;
	const effectiveWaitlistThankYou =
		waitlistThankYou ??
		signupConfig.waitlist_thank_you ??
		DEFAULT_WAITLIST_THANK_YOU;

	// ?waitlisted=true: user just authed but their account is on the
	// waitlist. Show only the thank-you message.
	if (waitlistedView) {
		return (
			<div className="login-page">
				<div className="login-card">
					<a href={`${__STATIC_ROOT__}/`} className="login-logo-link">
						<img src={resolvedLogo} alt={logoAlt} className="login-logo" />
					</a>
					<div className="login-success">{effectiveWaitlistThankYou}</div>
				</div>
			</div>
		);
	}

	return (
		<div className="login-page">
			<div className="login-card">
				<a href={`${__STATIC_ROOT__}/`} className="login-logo-link">
					<img src={resolvedLogo} alt={logoAlt} className="login-logo" />
				</a>

				{magicLinkSent ? (
					<div className="login-success">
						{magicLinkSentMessage ?? (
							<>
								<p>Check your email.</p>
								<p>
									We sent a sign-in link to <strong>{email}</strong>. Click
									it to continue. The link expires in 15 minutes and works
									once.
								</p>
							</>
						)}
					</div>
				) : (
					<>
						{showOAuth && (
							<>
								{providers.includes("google") && (
									<button
										type="button"
										className="btn btn-oauth btn-google"
										onClick={() => handleOAuthLogin("google")}
									>
										<svg viewBox="0 0 24 24" style={{ width: 20, height: 20 }}>
											<path
												fill="#4285F4"
												d="M22.56 12.25c0-.78-.07-1.53-.2-2.25H12v4.26h5.92c-.26 1.37-1.04 2.53-2.21 3.31v2.77h3.57c2.08-1.92 3.28-4.74 3.28-8.09z"
											/>
											<path
												fill="#34A853"
												d="M12 23c2.97 0 5.46-.98 7.28-2.66l-3.57-2.77c-.98.66-2.23 1.06-3.71 1.06-2.86 0-5.29-1.93-6.16-4.53H2.18v2.84C3.99 20.53 7.7 23 12 23z"
											/>
											<path
												fill="#FBBC05"
												d="M5.84 14.09c-.22-.66-.35-1.36-.35-2.09s.13-1.43.35-2.09V7.07H2.18C1.43 8.55 1 10.22 1 12s.43 3.45 1.18 4.93l2.85-2.22.81-.62z"
											/>
											<path
												fill="#EA4335"
												d="M12 5.38c1.62 0 3.06.56 4.21 1.64l3.15-3.15C17.45 2.09 14.97 1 12 1 7.7 1 3.99 3.47 2.18 7.07l3.66 2.84c.87-2.6 3.3-4.53 6.16-4.53z"
											/>
										</svg>
										Continue with Google
									</button>
								)}
								{providers.includes("github") && (
									<button
										type="button"
										className="btn btn-oauth btn-github"
										onClick={() => handleOAuthLogin("github")}
									>
										<svg viewBox="0 0 16 16" style={{ width: 20, height: 20 }} fill="currentColor">
											<path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0016 8c0-4.42-3.58-8-8-8z" />
										</svg>
										Continue with GitHub
									</button>
								)}
								{providers
									.filter((p) => p !== "google" && p !== "github")
									.map((provider) => (
										<button
											key={provider}
											type="button"
											className="btn btn-oauth"
											onClick={() => handleOAuthLogin(provider)}
										>
											Continue with {provider.charAt(0).toUpperCase() + provider.slice(1)}
										</button>
									))}
							</>
						)}

						{(effectiveSignupEnabled || effectiveWaitlistEnabled) && (
							<>
								{showOAuth && (
									<div className="login-divider">
										<span>or</span>
									</div>
								)}

								<form onSubmit={handleMagicLink} className="login-input-group">
									<input
										type="email"
										placeholder="Email A Magic Link"
										value={email}
										onChange={(e) => setEmail(e.target.value)}
										autoComplete="email"
										autoFocus={!showOAuth}
										required
										disabled={submitting}
									/>
									<button
										type="submit"
										className="btn btn-primary"
										disabled={submitting || !email}
										aria-label="Send magic link"
										title="Send magic link"
									>
										{submitting ? (
											"…"
										) : (
											<svg
												viewBox="0 0 16 16"
												width="16"
												height="16"
												fill="currentColor"
												aria-hidden="true"
											>
												<path d="M1.5 8a.75.75 0 0 1 .75-.75h10.69L9.22 3.53a.75.75 0 0 1 1.06-1.06l5.25 5.25a.75.75 0 0 1 0 1.06l-5.25 5.25a.75.75 0 1 1-1.06-1.06l3.72-3.72H2.25A.75.75 0 0 1 1.5 8Z" />
											</svg>
										)}
									</button>
								</form>

								{fields.length > 0 && (
									<div className="login-meta-fields">
										{fields.map((f) => (
											<input
												key={f.name}
												type={f.type ?? "text"}
												placeholder={f.label}
												value={meta[f.name] ?? ""}
												onChange={(e) =>
													setMeta((prev) => ({
														...prev,
														[f.name]: e.target.value,
													}))
												}
												required={f.required}
											/>
										))}
									</div>
								)}
							</>
						)}

						{(() => {
							const hasMessage =
								!!error ||
								effectiveWaitlistEnabled ||
								(!showOAuth &&
									!effectiveSignupEnabled &&
									!effectiveWaitlistEnabled) ||
								!!helperText;
							if (!hasMessage) return null;
							return (
								<>
									<div className="login-rule" />
									<div className="login-message">
										{error && <div className="login-error">{error}</div>}
										{effectiveWaitlistEnabled && (
											<div className="login-callout">
												{effectiveWaitlistMessage}
											</div>
										)}
										{!showOAuth &&
											!effectiveSignupEnabled &&
											!effectiveWaitlistEnabled && (
											<div className="login-helper">
												Sign-in isn't configured for this app. Contact an
												administrator for an invite.
											</div>
										)}
										{helperText && (
											<div className="login-helper">{helperText}</div>
										)}
									</div>
								</>
							);
						})()}
					</>
				)}
			</div>
			{bannerError && (
				<div className="login-banner-error">{bannerError}</div>
			)}
		</div>
	);
}
