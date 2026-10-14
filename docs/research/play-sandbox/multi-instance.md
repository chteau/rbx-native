# Running a sandbox client without disconnecting the live Roblox session

**TL;DR**
- Same account in two experiences at once is rejected by Roblox's backend (error 273/264, "same account launched experience from different device"); a second client on the *same account* is not a viable sandbox.
- The only clean way to keep the live game is a *second account* for the sandbox client, in an isolated client instance (separate Sober data dir / sandbox / second OS user).
- Multi-instance on Windows is technically achievable (singleton event/mutex), but Roblox staff have called it malicious [Reported]; on Sober it is unsupported upstream. Neither is officially sanctioned.
- Recommended: keep the roadmap design as is (one client at a time, same account), and offer an optional "sandbox profile" with a dedicated alt account. Do not ship multi-instance tricks in rbxstudio.
- Limits of this note: Roblox's help page was Cloudflare-blocked and the ToS text and the devforum staff post were not fetched, so the policy claims below are second-hand.

## Possible
- **Second account, separate session.** Two different accounts can be in two experiences at once; the 273/264 rule is per account. [Inferred] from the error's wording; see https://en.help.roblox.com/hc/en-us/articles/36665660855700-Error-Code-264-Same-account-launched-experience-from-different-devices (title only; body not fetched).
- **Windows multi-instance.** The client holds a singleton named object (`ROBLOX_singletonEvent`/`ROBLOX_singletonMutex`); tools close or pre-create it so a second client starts. [Reported] https://github.com/qwqtoday/multiblox-cs , https://github.com/sir49/SingleTonEventCloser . A Sober issue thread says Hyperion "doesn't seem to block it on Windows, just closing the ROBLOX_si…" (truncated) [Reported] https://github.com/vinegarhq/sober/issues/30 .
- **Bloxstrap native option.** Bloxstrap 2.9.0+ re-added "Allow multi-instance launching" [Verified, README] https://github.com/Zgoly/bloxstrap-multi-instance-integration/blob/main/README.md . Bloxstrap has open issues about it breaking https://github.com/bloxstraplabs/bloxstrap/issues/6054 .
- **Linux multi-instance.** Community tools run several Sober/Vinegar instances with separate accounts: https://github.com/Parham125/Linux-Roblox-Account-Manager , https://www.reddit.com/r/linux_gaming/comments/1aq6i65/how_to_run_multiple_roblox_instances_with_vinegar/ [Reported]. Vinegar had a multi-instance setting; Sober does not [Reported, sober#30].
- **Local tooling exists.** Sober is installed as Flatpak `org.vinegarhq.Sober` (stable, x86_64; `flatpak info`) [Verified locally]; `bwrap` is on PATH. [Inferred] A per-profile data dir via `flatpak run --env=...` or a second OS user / `bwrap --bind` of a private `$HOME` would keep cookies and settings separate.
- **Studio playtest does not touch the live session** in the sense that it never joins a real server: Studio Play/Run/Team Test run in Studio's own local or edit-session context [Inferred] from https://create.roblox.com/docs/studio/testing-modes ; not tested against the 273 rule.

## Not possible
- **Same account, two experiences.** The second launch kicks the first (error 273 for in-experience, 264 per the current help article) [Reported] https://devforum.roblox.com/t/teleporting-players-causes-error-273-same-account-launched-experience-from-different-device/4013867 and https://roblox-errors.fandom.com/wiki/Error_273 . No evidence that a private place, VIP/private server or reserved server is exempt; the check is account-level [Inferred].
- **A supported "second instance, same account" path.** None exists; no setting in Sober for it (sober#30 closed "not planned") [Reported].
- **Hiding the sandbox from Roblox via VM/Sandbox.** [Inferred] A VM or Windows Sandbox only isolates local state; the account-level kick is server-side, so it does not help for the same account. For different accounts it is unnecessary.

## Unknown / needs a live test
- Whether a Team Test or Studio *cloud* playtest (client connecting to a real cloud server with the Studio user) triggers 273 against a live game; not documented in what was fetched.
- Whether 273 fires when the second session is a private place in a different universe (expected yes, untested).
- Whether Sober still tolerates two `flatpak run` instances with different `--env=HOME`/bind-mounted data dirs, and where its cookies live (deliberately not inspected; `ls ~/.var/app/org.vinegarhq.Sober` was not read).
- Current Hyperion behaviour on Windows against a mutex-closing launcher in late 2026; and VM detection (anecdotal reports only, no primary source found).
- Exact ToS wording on alt accounts and multi-instance: ToS page not fetched. [Inferred] Alt accounts are common and not banned per se, but exploiting multiple accounts for in-game advantage violates experience rules.

## Verdict matrix
| Option | Keeps live game? | Works today | Ban/ToS risk | Verdict |
|---|---|---|---|---|
| Same account, second client (any OS) | No (273/264 kick) | Yes, but kicks | Low (just disconnect) | Reject as goal; it is the baseline roadmap behaviour |
| Same account, Windows singleton-event hack | No (server-side kick) | n/a | Medium: staff call multi-instance malicious [Reported] | Reject |
| Alt account, Windows + Bloxstrap multi-instance | Yes | Yes [Reported] | Medium: unsupported by Roblox, Hyperion risk; not a clear ToS ban | Do not integrate; document only |
| Alt account, Sober, separate data dir / bwrap / second Linux user | Yes [Inferred] | Untested | Low-medium: no tampering with client, but multi-instance unsupported | Best optional path, behind a setting |
| Alt account, VM / Windows Sandbox | Yes | Heavy | Medium-high: Hyperion VM concerns [Reported/unsourced] | Reject |
| Studio local playtest (no real server) | Yes | Yes | None | Fine for pure-Studio features, but not the sandbox |
| Second device (phone/other PC), alt or same | Alt yes, same no | Yes | None | Fully supported; manual |

## Account and profile design (if pursued)
- A "sandbox profile" in rbxstudio would only store a profile name, a launch command, and the sandbox place id; it must never read or store the Roblox cookie. The user logs into the alt inside the isolated client once. [Inferred]
- Linux launch: `flatpak run --env=HOME=<profile dir> org.vinegarhq.Sober roblox://placeId=…` is the idea, but Flatpak may ignore HOME; the reliable alternative is a second OS user or `bwrap`. Needs a live test.
- Publishing remains via the Open Cloud API key, independent of which account plays. The alt account needs permission to join the private place (add as collaborator/allow in the test universe). [Inferred]

## Impact on the roadmap design
- **Step 1 (private place):** note that the player account must be allowed into the place; with the same account the user will be kicked from any live game on launch. Document this in the Play UI (warn before launching).
- **Step 2 (launch client):** add an optional "sandbox profile" launcher (separate data dir/user, alt account) instead of any singleton-mutex tricks. Default stays: single client, same account, with a pre-launch warning.
- **Step 2 (embedding):** unaffected.
- **Steps 3-6:** unchanged; the probe/tunnel does not depend on which account runs the client.
- **New requirement:** a Play confirmation dialog: "This will disconnect you from any Roblox game you're in with this account."

## Sources
- https://en.help.roblox.com/hc/en-us/articles/36665660855700-Error-Code-264-Same-account-launched-experience-from-different-devices
- https://devforum.roblox.com/t/teleporting-players-causes-error-273-same-account-launched-experience-from-different-device/4013867
- https://roblox-errors.fandom.com/wiki/Error_273
- https://github.com/vinegarhq/sober/issues/30
- https://github.com/Zgoly/bloxstrap-multi-instance-integration/blob/main/README.md
- https://github.com/bloxstraplabs/bloxstrap/issues/6054
- https://github.com/qwqtoday/multiblox-cs
- https://github.com/sir49/SingleTonEventCloser
- https://github.com/Parham125/Linux-Roblox-Account-Manager
- https://www.reddit.com/r/linux_gaming/comments/1aq6i65/how_to_run_multiple_roblox_instances_with_vinegar/
- https://create.roblox.com/docs/studio/testing-modes
