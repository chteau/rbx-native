# Network simulator for Play on Windows and macOS

Research date 2026-10-17. Web only; no Windows or Mac machine was available, so nothing here was executed.

Labels:
- **[Verified: URL]**: read in a primary source.
- **[Reported: URL]**: secondary source or user report.
- **[Unknown]**: not found; must be tested on real hardware.

Short names for sources:
- DOC: https://raw.githubusercontent.com/basil00/WinDivert/master/doc/windivert.html
- HDR: https://raw.githubusercontent.com/basil00/WinDivert/master/include/windivert.h
- Clumsy: https://raw.githubusercontent.com/jagt/clumsy/master/src/ (`divert.c`, `lag.c`, `drop.c`)
- DNCTL_C: https://raw.githubusercontent.com/apple-oss-distributions/network_cmds/main/dnctl/dnctl.c
- DNCTL_8: https://raw.githubusercontent.com/apple-oss-distributions/network_cmds/main/dnctl/dnctl.8
- DUMMYNET: https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/netinet/ip_dummynet.c
- PFCONF5: https://keith.github.io/xcode-man-pages/pf.conf.5.html
- PFCTL8: https://keith.github.io/xcode-man-pages/pfctl.8.html

DOC and HDR are the `master` copies, not a 2.2.2 tag. [Unknown whether master differs from v2.2.2.]

## 1. Summary verdicts

### Windows: feasible with WinDivert, conditional on driver loading

- WinDivert is dual-licensed LGPLv3 or GPLv2. [Verified: DOC section 11 and the HDR licence header]
- The calling application must be Administrator. [Verified: DOC]
- At the NETWORK layer, `processId` and `localPort` are not filter fields. Only the `udp.SrcPort`/`udp.DstPort` header fields are available. [Verified: DOC filter fields table]
  - For an outbound packet the local port is `udp.SrcPort`. For an inbound packet it is `udp.DstPort`.
- The filter is fixed at `WinDivertOpen`. Changing ports means opening a new handle. [Verified: absence in HDR; Clumsy restarts its thread on filter change]
- Reinjecting unmodified packets with their original address needs no checksum work. [Verified: DOC, Clumsy]
- Delay in Clumsy is a FIFO release by `timeGetTime()`, and drop is a per-packet roll. [Verified: Clumsy `lag.c`, `drop.c`]
- Whether the 2.2.x driver loads on current Windows 11 with Secure Boot and Memory Integrity is [Unknown]. It is not known to be WHQL/attestation signed.
  - Third-party reports say it is signed by "Nemea Mjukvaruutveckling AB". [Reported: https://www.file.net/process/windivert64.sys.html]
  - A scan of a 2.2.2 `WinDivert64.sys` shows a valid signature. [Reported: https://fr.gridinsoft.com/online-virus-scanner/id/8da085332782708d8767bcace5327a6ec7283c17cfb85e40b03cd2323a90ddc2]
  - Many apps ship WinDivert 2.2.x on modern Windows, so it probably loads. [Reported, indirect]
- No source puts WinDivert 2.2.2 on the vulnerable-driver blocklist or calls it HVCI-incompatible. [Unknown]
- Whether Hyperion reacts is [Unknown]. No source mentions WinDivert or Clumsy causing a kick or crash. WinDivert never touches the Roblox process.

### macOS: feasible, with several grammar and reachability questions open

- `dnctl pipe N config` supports `bw`, `delay`, `plr`, `queue`, `mask` and `noerror`. [Verified: DNCTL_C]
  - `delay` is integer milliseconds only. The 0.1 ms Studio step rounds to 1 ms. [Verified: DNCTL_C]
  - `plr` is a float from 0 to 1, so 0.5 % is 0.005. [Verified: DNCTL_C, DNCTL_8]
- dummynet has no jitter and no `profile`. [Verified: DNCTL_C, DUMMYNET]
- Each pipe is FIFO and never reorders. Delay is applied at dequeue from a 1 ms clock. [Verified: DUMMYNET] The man page's "typically 10 ms" tick looks stale. [Verified: DNCTL_8 vs DUMMYNET; real granularity [Unknown] until measured]
- Re-running `dnctl pipe N config` on an existing pipe applies to new packets. Queued packets keep their old release time and nothing is dropped. [Verified: DUMMYNET `config_pipe`]
- pf cannot match a process, only a user or group. [Verified by absence in PFCONF5] Ports must be learned from the process.
- Whether Apple's pfctl accepts `probability` on `dummynet` rules is [Unknown]. `probability` is documented for filter rules. [Verified: PFCONF5]
- Whether a dummynet rule in `com.apple/rbxnative` is reached via the stock `dummynet-anchor "com.apple/*"` is [Unknown].
  - The stock `/etc/pf.conf` lists that anchor. [Reported: https://manjusri.ucsc.edu/2015/03/10/PF-on-Mac-OS-X/ ; https://developer.apple.com/forums/thread/745158]
  - The Sequoia file is not verified.
- Apple says Packet Filter is not an API, and warns against shipping a product on it. [Verified: https://developer.apple.com/forums/thread/776505 and TN3165, https://developer.apple.com/documentation/technotes/tn3165-packet-filter-is-not-api] That is acceptable for a developer tool, but expect breakage between releases.
- No report of dummynet removal on Apple Silicon or Sequoia was found, and `dnctl` and `ip_dummynet.c` are still on `main`. [Verified: DNCTL_C, DUMMYNET]
  - No working Sonoma/Sequoia recipe was found. [Unknown]
  - One user saw dummynet rules listed but no loss. [Reported: https://windows-hexerror.linestarve.com/q/su1536876-macos-isnt-using-my-configured-dummynet]

## 2. Chosen design

### Windows

- **Elevation.** The editor relaunches itself elevated with `ShellExecuteExW` (verb `runas`, hidden) and `--netsim-helper`.
  - This costs one UAC prompt per helper start. Cancelling returns `ERROR_CANCELLED` (1223) and shaping stays unavailable.
  - The helper is kept alive for the whole editor session, so the prompt appears once per session.
  - These are standard Win32 behaviours that I did not fetch. [Unknown as to URL; cite https://learn.microsoft.com/windows/win32/api/shellapi/nf-shellapi-shellexecuteexw on review]
- **IPC.** The unelevated parent creates the named pipe `\\.\pipe\rbx-netsim-<random>`, and the elevated helper connects as the client. The default DACL already admits the creator, so no custom DACL is needed.
  - The helper is an elevated process reading commands, so it accepts only the numeric line protocol in section 3. It never accepts filter strings or paths.
  - The helper exits when the pipe breaks or the parent process ends. It waits on the parent via `OpenProcess(SYNCHRONIZE)`.
  - An unelevated parent generally cannot terminate an elevated helper, so the helper must exit by itself. [Inference]
- **Capture.** One WinDivert NETWORK-layer handle is opened with flags 0. Never use SNIFF: it copies the packet and lets the original through, so there is no delay. [Verified: DOC flag semantics]
  - The filter is built from RobloxPlayerBeta.exe's local UDP ports, read from `GetExtendedUdpTable` (owner-PID UDP table). The helper never opens a handle to the Roblox process.
  - A filter is one `udp and (...)` expression: `(outbound and udp.SrcPort == P) or (inbound and udp.DstPort == P)` for each port.
  - The PIDs come from `tasklist /FI "IMAGENAME eq RobloxPlayerBeta.exe" /NH /FO CSV`.
  - A port-set change opens a new handle first, then calls `WinDivertShutdown(old, RECV)`.
  - Using `GetExtendedUdpTable` rather than a SOCKET-layer sniff handle avoids needing to open the handle before launch. [DOC: events before the handle opened are not captured]
  - The PID can be reused, so compare process creation time. [Verified: DOC known issues]
- **Shaping.** A receive thread calls `WinDivertRecv` immediately and moves each packet into a user-space FIFO delay line.
  - The release time is `max(now + latency + uniform[0, jitter], previous release)`, so packets are never reordered.
  - Loss is rolled on receipt, before queuing, as a Bernoulli trial per direction.
  - A sender thread reinjects due packets with `WinDivertSend` on the same handle, using the original `WINDIVERT_ADDRESS`. No checksum recalculation is done.
  - The driver queue limits (length 4096 default, max 16384; time 2000 ms default, max 16000 ms; size 4 MiB default, max 32 MiB) apply only to packets waiting for `Recv`. [Verified: HDR, DOC] Packets held in our own queue are not subject to them. A 1 s delay therefore costs memory, not driver drops.
  - Our own queue is capped at 16,384 packets. Beyond that, new packets are dropped.
  - The sender thread uses `timeBeginPeriod(1)` for 1 ms sleep granularity.
  - `WinDivertShutdown(h, RECV)` makes `Recv` fail with error 232 once the queue is empty, which stops the receive thread. [Verified: DOC]
- **Packaging.** `WinDivert.dll` and `WinDivert64.sys` sit beside `rbxstudio.exe`, taken from https://github.com/basil00/WinDivert/releases. The DLL is loaded dynamically at runtime.
  - The files are **not shipped** in this repository. The licence is LGPLv3 (the other half of the dual licence is GPLv2). Dynamic loading of an unmodified DLL fits LGPL.
  - If a user installs them, ship the licence text and state where the source is.
  - Official prebuilt drivers are already signed. [Verified: DOC]
  - The driver installs itself on the first `WinDivertOpen`. [Verified: DOC]
  - Whether the `.sys` must be beside the DLL when the DLL is loaded by absolute path is [Unknown].
- **Errors shown to the user** (from `GetLastError` after `WinDivertOpen`): [Verified: DOC]
  - 2 files missing; 5 not administrator; 87 bad filter.
  - 577 invalid driver signature; 654 another app's WinDivert driver is loaded.
  - 1060 driver not installed; 1275 driver blocked; 1753 Base Filtering Engine disabled.
  - For 1275, add a Memory Integrity hint. Microsoft describes the related dialog in https://support.microsoft.com/help/4526424/. [Verified]

### macOS

- **Elevation.** A root helper shell script is started once per editor session through `osascript` with `do shell script ... with administrator privileges`.
  - The command first copies the helper to a root-owned temp file with `mktemp` and `cat`, then runs that copy, so the user cannot swap the script after the prompt. The tail is backgrounded, so `osascript` returns at once.
  - The prompt text is "rbx-native wants to simulate network conditions for Play."
  - `AuthorizationExecuteWithPrivileges` is deprecated and `SMJobBless` needs Developer ID signing; both were rejected. [Reported/known; unverified here]
  - Behaviour of the backgrounded helper on Sequoia is [Unknown].
- **IPC.** Two FIFOs, `cmd` (parent to helper) and `status` (helper to parent), live in a 0700 directory created by the app. The helper validates each line as digits only and never executes anything it reads.
  - EOF on `cmd` ends the helper. A watcher also polls the parent PID once a second.
  - A trap on EXIT removes our rules and pipes.
- **pf.** Rules are loaded into the anchor `com.apple/rbxnative` without touching `/etc/pf.conf`.
  - `pfctl -E` prints a `Token : N` line, which is kept. `pfctl -X N` releases only our reference at exit. [Verified: PFCTL8] Never run `pfctl -d` or `dnctl flush`, because they would disturb other tools.
  - Cleanup is `pfctl -a com.apple/rbxnative -F all` plus `dnctl pipe delete` for our eight pipes. [Verified: the clear output, https://developer.apple.com/forums/thread/745158]
  - An emptied sub-anchor stays listed until reboot on Ventura and later, so the same anchor name is reused. [Reported: same thread]
- **Pipes.** Inbound uses 4201-4204 and outbound uses 4211-4214. These high numbers avoid collisions with Network Link Conditioner. [Reported: https://spin.atomicobject.com/simulating-poor-network-connectivity-mac-osx/]
- **Jitter.** Each direction gets four pipes with delays evenly spread over [latency, latency + jitter]. Cascading `probability` rules pick one per packet.
  - With four pipes, pipe i of K gets `probability 100/(K-i)%`, and the last has none. Because pf stops at the first `quick` match, this makes the choice uniform.
  - Pipes in different delay slots can reorder packets, as netem jitter does. [Unknown whether Studio's jitter reorders]
  - Every pipe in a direction carries the same `plr`.
  - If `pfctl -n` rejects `probability` on a `dummynet` rule, the helper falls back to a single pipe at the mean delay (latency + jitter/2) and reports `nojitter` on the status FIFO. The UI says jitter is unavailable. Do not use `mask`: it hashes per flow, not per packet.
- **Ports.** The app runs `pgrep -x RobloxPlayer` and `lsof -nP -a -p PID -iUDP -F n` every 500 ms. It sends a `ports` line only when the set changes.
  - The process name `RobloxPlayer` is [Unknown] until checked with `pgrep -x`.
  - The `lsof` format is [Unknown] on macOS; it is from man-page knowledge.
  - Rules: `dummynet out ... from any port { P } to any` and `dummynet in ... from any to any port { P }`.
  - Whether `from any port` is accepted on dummynet `in` rules is [Unknown].
  - pf creates state on the first packet of a flow. Whether the pipe attached to existing states can be retargeted is [Unknown], so start shaping before the game connects.
  - Roblox documents UDP 49152-65535 for in-experience traffic as firewall guidance, direction unstated. [Reported: https://en.help.roblox.com/hc/en-us/articles/203312840-Firewall-and-Router-Issues] Ports are therefore learned, never hard-coded.

## 3. Line protocol

The parent sends one `\n`-terminated line per command. The helper accepts only digits and single spaces, with no sign and no leading zero except `0` itself.

| Line | Meaning |
| --- | --- |
| `out <latency_us> <jitter_us> <loss_ppm>` | Outbound conditions. |
| `in <latency_us> <jitter_us> <loss_ppm>` | Inbound conditions. |
| `ports <p1> ... <pn>` | macOS only. 0 to 64 ports, each 1 to 65535. |
| `stop` | The helper cleans up and exits. |

- Latency and jitter run 0 to 1,000,000 µs. Loss runs 0 to 5000 ppm (0.5 %).
- macOS rounds microseconds to whole milliseconds for `dnctl`, and prints loss as `0.%06d`.
- The Windows pipe carries only `out`, `in` and `stop`. Ports come from the helper's own `GetExtendedUdpTable` scan.

Replies from the helper:
- macOS (over the `status` FIFO): `ready <pid>`, `nojitter`, `ok out`, `ok in`, `ok ports`, `error <verb> malformed`, `error <verb> dnctl`, `error ports pfctl`, `error start dnctl`, `error unknown`.
- Windows (over the pipe): `ready`, `error <code> <text>`, `ok out`, `ok in`, `error out <text>`.

## 4. What is unverified

- All behaviour on real hardware. Every item in the checklists below is untested.
- WinDivert on Windows:
  - Driver loading under Secure Boot, Memory Integrity and the blocklist.
  - The search rule for the `.sys` file.
  - Which UDP sockets RobloxPlayerBeta opens (voice, telemetry), and so what the filter covers.
  - Hyperion's reaction.
  - AV detection rates for the 2.2.2 driver.
- `WINDIVERT_ADDRESS` layout (my inference, not from the header). The 80-byte size follows from 8 + 4 + 4 + 64. The `Outbound` flag is bit 17 of the bitfield dword, assuming LSB-first allocation.
- The behaviour after the last handle closes, and whether the `WinDivert` service stays loaded until reboot. [Unknown]
- macOS:
  - `probability` and `port {...}` grammar on `dummynet` rules.
  - Reachability of the sub-anchor through the stock ruleset on Sequoia.
  - Whether a customised `pf.conf` lacks `dummynet-anchor "com.apple/*"`. In that case the fallback would be reloading the main ruleset with extra anchor lines. [Reported: https://docs.rs/msg-sim]
  - Real delay granularity, and `lsof` output for RobloxPlayer.
  - Sequoia 15.0-15.3.1 reportedly let some outbound packets bypass `block all`, so coverage may be incomplete. [Reported: https://developer.apple.com/forums/thread/776914]
- Hyperion on macOS: nothing technical found. Expected risk is low because shaping happens in the kernel network stack, outside the process. [Unknown]
- Roblox's own Studio Network Simulator reportedly does not trigger the same Server Authority desyncs as external tools such as Clumsy. [Reported: https://devforum.roblox.com/t/recent-network-simulation-settings-causing-irregular-resimulations-on-server-authority/4708760]

## 5. Live-test checklist

### Windows 10/11 box

1. Place `WinDivert.dll` and `WinDivert64.sys` from https://github.com/basil00/WinDivert/releases beside `rbxstudio.exe`. Confirm the driver loads with Secure Boot and Memory Integrity on. Record the outcome for errors 577, 1275 and 654.
2. Confirm the `.sys` is found when the DLL is loaded by absolute path.
3. Run the UAC flow: accept it, then cancel it. Cancel should give error 1223 and a clear "shaping unavailable" message. Confirm there is one prompt per editor session, not per Play.
4. Confirm the pipe flow: the helper replies `ready`, and `out`/`in` lines get `ok out` and `ok in`.
5. Quit the editor normally, then kill it from Task Manager. In both cases the helper must exit and the WinDivert handle must close. Check that no stray `rbxstudio.exe` remains.
6. Check `size_of::<Address>() == 80`. Confirm bit 17 matches `Outbound` by logging a few packets.
7. Launch the player and compare the `GetExtendedUdpTable` ports against a packet capture. List every UDP socket RobloxPlayerBeta opens (voice, telemetry, QUIC) and decide what the filter should cover.
8. Measure with `ping`, or a RTT probe in a test place, under each preset:
   - Latency: the added RTT should match the sum of both directions, within about 1 ms.
   - Loss: over a long run it should be near the configured rate (at most 0.5 %).
   - Jitter: RTT variance should match the range, and a trace should show no reordering.
9. Change sliders live. Confirm there is no packet burst and no stall.
10. Change the port set by relaunching the player. Confirm the filter handle swap leaves no unshaped gap that matters.
11. Use a 1000 ms delay on a busy flow. Confirm memory stays bounded and the 16,384-packet cap drops cleanly.
12. Run a short session on a throwaway account. Watch for Hyperion kicks or crashes.
13. Check other apps' UDP traffic is untouched while shaping.
14. After stopping, check `sc query WinDivert` for a leftover service and note whether it needs `sc stop`.

### Apple Silicon Mac, Sequoia

1. Run `echo 'dummynet out quick proto udp from any to any probability 50% pipe 4211' | pfctl -n -a com.apple/rbxnative -f -`. Confirm it is accepted, and note `port {...}` and `from any port` on `in` rules too. If it is rejected, confirm the single-pipe fallback and the `nojitter` reply.
2. Check that the rules shape traffic with only the stock `pf.conf`: `pfctl -a com.apple/rbxnative -sr`, and `dnctl pipe show` counters.
3. Run the password prompt flow. Accept it and cancel it. Check the dialog text, and check that the helper survives `osascript` returning.
4. Quit the editor normally and by SIGKILL. The helper should see EOF or the parent-PID watcher and clean up. Check with `pfctl -a com.apple/rbxnative -sr`, `dnctl list` and `pfctl -s References`.
5. Start the helper twice, with a leftover from a killed run. Check that startup cleanup is idempotent.
6. Confirm `pgrep -x RobloxPlayer` finds the client, and read the real `lsof -nP -a -p PID -iUDP -F n` output against `parse_lsof_udp`.
7. Check whether an already-open flow picks up a pipe when rules are loaded late.
8. Measure real delay granularity, and confirm `dnctl pipe N config` changes the delay of a running `ping` without drops.
9. Measure RTT with `ping` or a probe under each preset. Compare added RTT against the configured latency (rounded to 1 ms) and loss against the configured rate. Check that jitter spreads over the four pipe delays.
10. Check that pf stays enabled for other holders: `pfctl -s References` after `-X`. Confirm `pfctl -d` was never run.
11. Run Network Link Conditioner alongside to confirm there are no pipe-number collisions.
12. Run a short session with the Roblox client under shaping. Watch for Hyperion kicks, and for packets that bypass pf on Sequoia.
