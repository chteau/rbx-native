# Co-editing without an rbx-native host

Measured 2026-10-29 in Roblox Studio 0.745 (Vinegar, Linux) against the
TeamPresence plugin and `rbx_presence`. Covers where the co-editing op log
lives when nobody runs rbx-native, how a new host picks it up, and why the
12,000 part initial sync took about a minute.

## Where the log lives

Three transports, three answers:

- **ntfy.sh** carries no log. Its free limits (message size, cache time,
  per-IP rate) rule it out for an ordered, replayable log, so co-editing
  over ntfy stays off and the plugin says so.
- **The hosted relay** (`rbx_presence::host` behind a quick tunnel) keeps
  the log in memory. It dies with the host, and only an rbx-native user can
  run one.
- **The Worker** (`relay/`) keeps the log in its Durable Object's SQLite
  storage, as ciphertext only, so Studio users alone can co-edit with no
  rbx-native editor anywhere. The free plan's 100,000 rows written per day
  is the only real limit: one logged chunk is one row, the 12,000 part
  place below is 632 rows, so about 150 such syncs a day per account.

No host election is needed: the relay is the one authority on order (its
`seq`), on every transport that carries a log. What it lacks is
durability, and every peer already holds a copy.

## Handover

Each peer keeps the entries it read, as logged, plus the log's id (a random
id the relay mints on the first post). When a peer reaches a relay:

- same log, relay ahead or equal: read on;
- relay empty, or the same log behind what the peer holds: the peer hands
  over (`POST /ops/fill`) what the relay lacks, seqs and authors intact;
- a different log: the peer drops its copy and reads the new one from 0.

A fresh host refuses new posts (`409 log`) for 30 s after its first request
so a peer holding the old log can fill it first, instead of a new log
racing in and orphaning everyone's history. Fill entries are re-posted
ciphertext; the AEAD binds room, user and client, not `seq`, so they still
open. Both the Rust channel (`coedit::keep`) and the plugin
(`CoeditKept.luau`) run the same rule; the tests are
`tests/coedit_e2e.rs` (`the_next_host_takes_over_the_log`,
`an_empty_host_waits_for_the_peer_that_holds_the_log`),
`host/ops/tests.rs`, `relay/test/ops.test.ts` and
`tests/luau_coedit_kept.rs`.

## The 12,000 part sync

The test: the command bar makes a `Folder` of 12,000 anchored Parts in
Studio (0.26 s), the plugin syncs it, and a headless `rbx_presence` peer on
the same machine reads and applies it. Local relay, so no network round
trip; the machine's load average was 20 to 30 throughout.

| | before | after |
|---|---|---|
| Log entries (7000 byte chunks) | 632 | 632 |
| Ciphertext | 7.92 MB | 7.92 MB |
| POSTs | 632 | 16 (512 KiB groups) |
| Entries landing in the log | over 29.0 s | over 2.3 s |
| Created to fully received | 29.5 s | about 12 s |
| Peer apply (12,001 ops) | not measured | 4.3 s |

The bottleneck was one signed POST per chunk: about 46 ms each, paid 632
times, plus the round trip each time. Through a quick tunnel that round
trip is what made it a minute. Now the plugin seals every chunk first and
posts them in groups of up to 512 KiB of ciphertext (the relay takes 1 MiB
bodies on the ops routes), so the round trip is paid 16 times instead of
632. At 50 ms a round trip that's about 0.8 s against about 32 s.

What's left is the plugin: about 9.5 s between the parts existing and the
first entry landing, collecting 12,000 instances and sealing 7.9 MB with
pure-Luau ChaCha20-Poly1305. The received side's 4.3 s is applying 12,001
ops to the DOM.

Studio documents a 500 requests per minute HttpService limit. The old path
made about 1300 a minute and was never throttled from a plugin, but 16
requests keeps far clear of it either way.

## Not measured

- The handover between two real hosted relays (two rbx-native editors
  through cloudflared): covered by the e2e tests over real HTTP, not run
  live.
- The 12,000 part sync through a real tunnel after the change; the round
  trip saving above is computed, not measured.
- A deployed Worker on Cloudflare: Studio-only co-editing ran against
  `wrangler dev --local` with its SQLite storage restarted between joins.
