# Tunnel options for the probe HTTP server (step 4)

**TL;DR**
- Roblox game servers can only reach public HTTPS endpoints, so the editor needs a tunnel. Candidates are no-account quick tunnels (cloudflared, pinggy, localtunnel) and account-based or self-hosted ones (Tailscale Funnel, zrok, ngrok, frp, rathole, sish, bore, Pangolin).
- Default: **cloudflared quick tunnel** (Apache-2.0, no account, HTTPS, Linux and Windows). Fallback: **Tailscale Funnel** (stable `*.ts.net` URL; Tailscale is already installed on the maintainer's machine).
- The URL is public whichever tunnel is used. Authenticate every probe request with a per-session random token in a header.
- Nothing here was exercised against a live Roblox server. Several facts come from memory and are tagged [Inferred] or [Reported]; the live-test list is below.

## Possible

Local read-only probe: `tailscale version` gives 1.102.4, and `tailscale funnel --help` shows `funnel [--bg] <port|url>`, `funnel status [--json]` and `funnel reset`. [Verified] (local CLI)

| Tool | License | Account? | Stable URL | HTTPS | Interstitial | Caps | Self-host | Win+Linux |
|---|---|---|---|---|---|---|---|---|
| cloudflared quick tunnel | Apache-2.0 [Verified] (repo) | No | No, random `*.trycloudflare.com` per run | Yes | None for API requests [Inferred] | 200 in-flight requests; no SSE [Reported] | No (Cloudflare edge) | Yes |
| Tailscale Funnel | Client BSD-3; service proprietary | Yes (tailnet) | Yes, `host.tailnet.ts.net` | Yes, `*.ts.net` cert | None | Bandwidth limits, not precisely published [Reported] | No | Yes |
| zrok | Apache-2.0 (OpenZiti) | Yes (zrok.io account) or self-host | Reserved shares are stable | Yes | None on public shares [Inferred] | Hosted fair-use [Inferred] | Yes | Yes |
| ngrok | Closed agent | Yes | Free tier: one static domain [Reported] | Yes | Browser-warning page unless `ngrok-skip-browser-warning` header or custom User-Agent is sent [Reported] | Free-tier caps | No | Yes |
| bore | MIT | No (`bore.pub`) | No | **No**, raw TCP | None | None published | Yes | Yes |
| frp | Apache-2.0 | Own VPS | Yes | Yes with own cert | None | Your VPS | Yes | Yes |
| rathole | Apache-2.0 | Own VPS | Yes | Not built in; front with Caddy/nginx [Inferred] | None | Your VPS | Yes | Yes |
| sish | MIT | Self-host, or pico.sh (SSH key) | Yes with custom domain | Yes | None | Hosted: unknown | Yes | Server Linux-oriented; client is plain `ssh` |
| localtunnel | MIT | No | Random or requested subdomain | Yes | Reminder page wanting the tunnel password unless `bypass-tunnel-reminder` header is sent [Reported] | Often unreliable [Reported] | Yes | Needs Node |
| pinggy | Service proprietary; client is `ssh` | No for short free tunnels [Reported] | Paid plans only | Yes | Unknown | Free tunnels time out [Reported] | No | Yes |
| Pangolin | AGPL-3.0 (community edition) [Reported] | Self-host | Yes | Yes (Traefik) | None | Your VPS | Yes | Server Docker/Linux; Newt client both |

Why cloudflared first:
- One static binary, no login, so Play works with no extra setup.
- `cloudflared tunnel --url http://127.0.0.1:PORT` prints the `https://<words>.trycloudflare.com` URL on stderr, which rbxstudio can scrape. [Reported]
- A random URL per run is unguessable, which helps a little but is not authentication.

Why Funnel as fallback:
- Needs HTTPS on 443, 8443 or 10000, a `*.ts.net` cert, and the `funnel` node attribute in the tailnet policy. [Verified] (Tailscale Funnel docs)
- Stable URL, one-line command, `funnel reset` to tear down. [Verified] (local `--help`)
- The user must be logged in to Tailscale and have Funnel enabled in the admin console; rbxstudio cannot do that for them.

Self-hosted options (frp, rathole, sish, Pangolin) suit users with a VPS. Offer them through a "custom command" setting, not as defaults.

## Not possible

- **Raw TCP tunnels**: `HttpService` is HTTP(S) only, so bore cannot be used as is. [Inferred]
- **SSE through quick tunnels**: Cloudflare documents a 200 concurrent request limit and no SSE for quick tunnels. [Reported] The design uses long-polling, which is ordinary HTTP. Keep each long-poll to ~25 s so no idle timer cuts it (real timeout unknown).
- **Domain blocking**: `HttpService` blocks Roblox's own domains. [Reported] (DevForum) I found no reports of `trycloudflare.com`, `ts.net` or ngrok domains being blocked. [Inferred] from absence; needs a live test.
- **ngrok and localtunnel without vendor-specific headers**: both show a warning page by default. That couples the probe to the vendor, so neither is a good default.

## Unknown / needs a live test

1. A real `HttpService:RequestAsync` from a published place to a trycloudflare URL; no source confirms this end to end.
2. How long a long-poll can be held on each tunnel.
3. Added latency against the ~200 ms batching interval. [Inferred] tens of ms, not measured.
4. Whether Funnel throttling affects small JSON diffs. [Inferred] unlikely.
5. Whether the ~500 req/min `HttpService` limit counts long-polls and posts together; 200 ms batching alone is 300/min.
6. Whether any bot protection on a tunnel rejects Roblox's User-Agent.

## Authenticating the probe

1. **Per-session token**: rbxstudio generates 128+ random bits at Play, injects it into the probe (in-memory DOM only), and the probe sends it as `X-Probe-Token`. The server compares in constant time and answers 404 (not 401) otherwise.
2. Bind the server to 127.0.0.1; the tunnel process is the only way in.
3. Session id plus sequence number per message, so replays are dropped.
4. Body size and rate limits, and a hard timeout that stops server and tunnel when Play ends.
5. Optional HMAC of the body with the session secret: TLS already covers confidentiality, but Cloudflare and Tailscale terminate TLS, so this stops tampering at the edge.
6. The token is inside the published place, so make it session-scoped: new per Play, never reused. The place is private and anonymized, so exposure is limited. [Inferred]
7. Script source and instance diffs pass through the tunnel vendor; say so in the UI.

## How rbxstudio would manage the tunnel

- A `TunnelProvider` trait: `start(local_port) -> Result<PublicUrl>` and `stop()`. Implementations: `Cloudflared`, `TailscaleFunnel`, `CustomCommand` (user template plus a regex that extracts the URL).
- Spawn with `std::process::Command`, read stderr lines until the URL appears or a ~20 s timeout. Kill the child on Play stop, window close and panic (guard type with `Drop`).
- Funnel: prefer the foreground `tailscale funnel <port>` held as a child, because `--bg` state is machine-wide and persists; if `--bg` is used, call `tailscale funnel reset` only for config rbxstudio created. [Inferred]
- Locate binaries on PATH, then a settings path. Offer a one-click download of the official cloudflared release with checksum verification rather than silently bundling.
- Health check: GET `/healthz` through the public URL with the token before publishing, so a broken tunnel fails before the slow publish.
- Settings: provider, custom command, keep tunnel between sessions (off by default).

## Impact on the roadmap design

- **Step 4**: replace "Cloudflare quick tunnel" with a pluggable provider, cloudflared by default and Funnel as fallback. Keep HTTP-only long-polling with ~25 s polls. Add the token check, 404 on mismatch, loopback bind and teardown timeout.
- **Step 3**: the probe must carry the token and public URL, so the tunnel must be up before publish. Order: start server, start tunnel, health check, inject, publish (step 1).
- **Step 5**: unchanged; the per-run URL leaves nothing stable to anonymize beyond place metadata.
- **Step 6**: unchanged; hot-reload payloads use the same token.
- **Open item**: check the ~500 req/min budget against batching plus long-polling.

## Sources

- https://github.com/cloudflare/cloudflared
- https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/do-more-with-tunnels/trycloudflare/
- https://tailscale.com/kb/1223/funnel
- https://tailscale.com/kb/1247/funnel-serve-use-cases
- https://openziti.io/docs/learn/quickstarts/zrok/
- https://github.com/ekzhang/bore
- https://github.com/fatedier/frp
- https://github.com/rathole-org/rathole
- https://github.com/antoniomika/sish
- https://github.com/localtunnel/localtunnel
- https://pinggy.io/
- https://github.com/fosrl/pangolin
- https://ngrok.com/docs/network-edge/interstitial/
- https://create.roblox.com/docs/reference/engine/classes/HttpService
