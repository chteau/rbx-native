# Update planning: how an experience icon is set and moderated

Source: `Roblox/creator-docs` `content/en-us/reference/cloud/openapi.json`
(fetched 2026-10-28). Nothing here has been sent to Roblox yet; every
"works" below is what the spec says, not a live result.

## Setting the icon

| Route | Auth | Notes |
| --- | --- | --- |
| `POST apis.roblox.com/legacy-game-internationalization/v1/game-icon/games/{universeId}/language-codes/{languageCode}` | API key (`x-api-key`) or OAuth, scope `legacy-universe:manage` | multipart, one `Files` part holding the image |
| `GET  apis.roblox.com/legacy-game-internationalization/v1/game-icon/games/{universeId}` | same | `data[]` of `{imageId, imageUrl, state, languageCode}` |
| `POST gameinternationalization.roblox.com/v1/game-icon/games/{gameId}/language-codes/{languageCode}` | `.ROBLOSECURITY` cookie | the same call, web-signed |

So an API key **can** set the icon: the `legacy-*` routes are the web
endpoints re-exposed on `apis.roblox.com` behind a key scope. The editor uses
the key route through `rbx_cloud` (`game_icon.rs`), and the key needs the
`legacy-universe:manage` permission on top of what it already has; the setup
wizard lists it as optional ("Update planning").

Open questions only a live call answers:

- The POST's documented errors include `26: You can't update translations
  for source language`. That code is shared with the badge and pass routes;
  whether the game icon refuses the experience's *source* language (the icon
  everyone sees) is unknown. If it does, the only other route is the
  web-signed one above (same refusal likely) or the Creator Hub's own call,
  which is not in the spec. The code sign-in session (`rbx_play`'s Quick Log
  In, kept by `key_store::read_session`) is there to fall back on.
- The `{gameId}` path parameter is the universe id on every
  `game-internationalization` route we already know; assumed the same here.

## "Approved" ahead of time

Posting to the icon route *sets* the icon, so it can't be used to have the
countdown images reviewed early. Instead **Start update** uploads each image
as a `Decal` through the Assets API (`POST /assets/v1/assets`, scope
`asset:write`, the path the Figma import already uses) and polls
`GET /assets/v1/assets/{id}?readMask=moderationResult`, whose
`moderationResult.moderationState` is `Reviewing`, `Rejected` or `Approved`.

Whether an approved decal makes the later icon post of the *same bytes* skip
review is not documented. After a swap the icon's own state comes back from
the icon GET (`Approved`, `PendingReview`, `UnAvailable`, `Rejected`,
`Error`), which the steps view shows.

## Firing

Steps are "set the icon the plan says is due now", so a run that comes late
(the PC was asleep) applies the latest due image and marks the earlier ones
skipped. They fire while the editor is open, and from a helper the OS runs
every minute: `rbxstudio --update-planning-tick`, registered as a systemd
user timer (Linux), a Task Scheduler task (Windows) or a launchd agent
(macOS), and removed when no plan is left running.
