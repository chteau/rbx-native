# Play/Test sandbox: policy and risk review

**TL;DR**
- Nothing in the design is inherently a ToS violation: publishing through Open Cloud with your own key, `HttpService` calls, and `loadstring` in your own experience are all documented features. [Verified for loadstring/HttpService via creator-docs; Inferred for the ToU.]
- The real risks are behavioural: automated churn of publishes, input injection into the client, an unofficial client (Sober), and a public tunnel. None is clearly banned, none is explicitly blessed.
- Sober states it is unsupported by Roblox and experimental. [Verified: https://sober.vinegarhq.org]
- Biggest engineering risk is the tunnel plus `loadstring`: together they are remote code execution on the sandbox server. Mitigate with a per-session secret, HMAC and short TTL.
- Caveat: the research web tool returned no search results and Roblox's help site was behind a Cloudflare challenge. Roblox ToU/Community Standards wording below is therefore NOT quoted from a page I read; verify before relying on it.

## Possible
- **Open Cloud publishing**: the Place Publishing API exists for automating publishes, with API keys scoped to `universe-places:write` and a specific universe. [Reported/Inferred from the Open Cloud docs: https://create.roblox.com/docs/cloud/reference/features/universes-and-places] Use is by design; keep the call rate low (one publish per session, per the roadmap).
- **loadstring**: `ServerScriptService.LoadStringEnabled` is a documented, supported property. Docs warn it "can lead to remote code execution vulnerabilities" and recommend keeping it off. [Verified: creator-docs `ServerScriptService.yaml`, https://create.roblox.com/docs/reference/engine/classes/ServerScriptService] A private, owner-only test place is the intended-safe context.
- **External HTTP**: `HttpService` is documented, requires `HttpEnabled`, and rate-limits (~500 req/min). [Verified in creator-docs: https://create.roblox.com/docs/reference/engine/classes/HttpService] Requests to arbitrary domains are allowed; Roblox's own domains are blocked.
- **Anonymized private places**: randomized names and no icon on a private (non-public) place. [Inferred] Community Standards target deceptive public experiences (misleading titles/thumbnails, impersonation). A private place nobody else can find is not publicly deceptive, and an empty description is not prohibited that I know of. Moderation of names still applies: random names must come from a clean wordlist (hex or numbers, not words).
- **Inactive places**: [Unknown] I found no rule deleting inactive private places; places are not known to expire. Treat as not a risk but unconfirmed.
- **Monetization mocks**: faking `MarketplaceService.ProcessReceipt` in your own private place is just Luau running in your own server. [Inferred] ProcessReceipt is a callback you author; the engine only calls it for real purchases. A mock that calls your own handler with fake receipts never touches Roblox's payments. Rule of thumb: never grant real-world value, never run mocks in a public/live place, and gate mock code on the sandbox flag so it cannot ship.
- **One's own account** can run the sandbox place: the owner can always join their private place. [Inferred]

## Not possible
- Launching a second Roblox client session under the same account without kicking the live one: Roblox allows one active game session per account and joining another disconnects the first. [Reported, widely known; see the sandbox-session note for detail.]
- Rate-unlimited publishing: Open Cloud has per-key quotas. [Inferred]
- Getting official support for Sober. [Verified: "not supported by Roblox and may be discontinued at any time", https://sober.vinegarhq.org]

## Not clearly allowed (grey zones)
- **Unofficial client (Sober)**: Roblox's ToU forbids tampering with or reverse engineering the client and using unauthorized software that gives unfair advantage. [Inferred/uncertain] Sober runs the real, unmodified Android client in a runtime, is closed source, and has been publicly available for years; Roblox has not been reported to ban for it, but this is not a guarantee. The design's use does not need cheats. Recommend: warn the user in settings, never ship Sober, and let the user opt in.
- **Alt accounts**: ToU permits only a user's own accounts; multiple accounts are not banned outright but ban evasion is. [Inferred] A dedicated test account is the safest way to avoid disconnecting the main session, but avoid creating many accounts quickly, do not share them, and never use them to evade a ban.
- **Automating the client (window embedding, input injection)**: X11 reparenting and XTest synthetic input are OS-level and invisible to Roblox, but injecting input into the Roblox client is automation of the client, and the ToU forbids "automated means" for gameplay advantage. [Inferred] Keep it to test navigation in your own private place; do not automate public games. Prefer driving the game from the in-game probe (RemoteEvent) rather than injecting OS input. On Windows, injecting into or hooking `RobloxPlayerBeta.exe` risks anti-cheat (Byfron/Hyperion) flags. [Reported] Reparenting a window with no memory access is far safer than DLL injection; never do the latter.
- **Publishing a modified copy**: the probe is injected only into the in-memory DOM, so the user's real file stays clean. Leaking the probe into the saved place or a public place would be a real harm (an open RCE endpoint).

## Unknown / needs a live test
- Whether Roblox moderation flags a random-named, icon-less private place, or throttles repeated overwrites.
- Open Cloud per-key publish rate limits and any daily caps for `publish_place` (client written, never exercised).
- Whether `HttpService` to `*.trycloudflare.com` / `*.ts.net` hosts is accepted by the engine's request filter. Roblox blocks some domains; confirm with a test.
- Whether Roblox's current ToU text bans Sober-class runtimes or includes language on "emulators".
- Whether a test account triggers phone/ID verification when joining a private place.
- Whether a live play session on the main account is dropped when the sandbox joins on a second device.

## Open Cloud API key handling
- Least-privilege scopes for this design: `universe-places:write` (publish) restricted to the **test universe only**. Nothing else: no `universe:write`, no datastores, no asset write. The roadmap's other features (game settings, key wizard) request more; the sandbox should use a separate key, not the broad one. [Inferred]
- Restrict the key to the test universe, set an expiry, and optionally add an IP allowlist (the tunnel is irrelevant to this; publishing is outbound from the editor).
- Store in the OS keyring (already done); never write it into the injected probe, the published place, logs or crash dumps. The probe must never receive the Open Cloud key: it only gets the session token.
- Treat a leaked key as publish access to the test universe only, which bounds the damage if scoped well.

## Threat model: tunnel to a local server
Assets: the developer's machine, the Open Cloud key, the in-memory DOM, the ability to run Luau (via `loadstring`) in the sandbox server.

| Threat | Mitigation |
|---|---|
| Anyone who learns the tunnel URL can hit the local server | URL is a random subdomain (quick tunnel) or an unguessable path; plus a per-session bearer token injected in the probe; reject without it, constant-time compare |
| Command channel = RCE in sandbox server | HMAC-sign each command with a per-session key; reject replays (nonce/seq); sandbox place is private and holds no secrets |
| Server serves editor data to the internet | Bind to 127.0.0.1 only; expose only `/probe/*` endpoints; never serve files, the DOM wholesale or the keyring |
| Probe token extractable by a joined player | Private place, owner only; token lives in a server-only `Script` in ServerScriptService (never replicated); client mirror gets no token |
| Request flooding / body bombs | Body size cap, rate limit about 10 req/s, timeouts, drop unknown content types |
| Tunnel provider sees plaintext | Traffic to the provider terminates TLS there. [Inferred] Keep payloads non-sensitive (property diffs, not credentials); optionally encrypt bodies end to end with the session key |
| Stale tunnel after quit | Tear the tunnel and server down on exit; rotate token each session |
| Tailscale Funnel exposure | Funnel publishes a public URL on your tailnet's `*.ts.net` name for the node; it uses the signed-in account and exposes only the chosen port. Account-level ACL needed to enable. [Reported; verify in Tailscale docs: https://tailscale.com/kb/1223/funnel] |
| Local port reachable by other local users | Use a random high port and token; or unix socket behind the tunnel client where supported |

## Impact on the roadmap design
- **Step 1**: use a dedicated scoped key (`universe-places:write`, test universe only) and an optional test account; add a "do not publish into a universe you don't own / not the test universe" guard.
- **Step 2**: keep launching the unmodified official client; document Sober as unsupported by Roblox; prefer probe-driven control over OS input injection; embedding by reparenting is OS-level and low-risk, no DLL injection on Windows.
- **Step 3**: also inject a per-session token and a sandbox flag; refuse to run if the place is not the configured sandbox place ID. Never write the injection to disk.
- **Step 4**: add token + HMAC + replay protection + loopback bind + rate limit as hard requirements; tunnel choice (Tailscale Funnel vs Cloudflare) matters less than these.
- **Step 5**: random names from hex, private visibility; no known rule against it for private places.
- **Step 6**: `loadstring` is documented but dangerous; only valid with step 4 hardening.
- **Monetization testing mock**: allowed for private-sandbox use; gate behind the sandbox flag so mocks cannot reach a published experience, and document that results are not proof of real purchase behaviour.
- **Session conflict**: recommend a test account and/or a second device rather than trying to hack around single-session limits.

## Sources
- https://sober.vinegarhq.org
- https://create.roblox.com/docs/reference/engine/classes/ServerScriptService
- https://create.roblox.com/docs/reference/engine/classes/HttpService
- https://create.roblox.com/docs/cloud/reference/features/universes-and-places
- https://create.roblox.com/docs/reference/engine/classes/MarketplaceService
- https://tailscale.com/kb/1223/funnel
- https://en.help.roblox.com/hc/en-us/articles/115004647846-Roblox-Terms-of-Use (fetch blocked by Cloudflare; not read)
- https://en.help.roblox.com/hc/en-us/articles/203313410-Roblox-Community-Standards (not read)
