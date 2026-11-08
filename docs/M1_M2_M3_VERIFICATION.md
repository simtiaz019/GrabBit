# M1, M2 and M3 verification

Date: 2026-11-08 (Asia/Dhaka)

## Builds tested

- Source: commit `4f7421a` plus the preserved uncommitted M1/M2/M3 work and the IPC shutdown fix documented below.
- Fresh test executable: `target/release/grabbit-desktop`, SHA-256 `7b68743e46ea45c5725261399feac75fd8f8b9c6c2f4fa18f5b6194cff8ca15e`.
- Installed application: `/Applications/GrabBit.app`, version `0.1.0`, executable SHA-256 `22464f707997930765ab89230ce608b6cbf6dc2765eb146d32a7da1c6c8dc1a0`.
- The installed Chromium/Firefox extension resources exactly matched the working-tree build, and the installed executable contained the M3 tray menu. It does **not** contain the later IPC shutdown fix and is therefore outdated relative to the final tested source.
- Browsers: Brave `155.1.97.56`; Chrome `156.0.8078.80`.

## Result matrix

| ID | Feature | Automated | Real Browser | Result | Notes |
|----|---------|-----------|--------------|--------|-------|
| M1-1 | Native connection and durable capture | Native Messaging + local HTTP | Brave | PASS | Real `PING`, accepted job, 8 MiB completed file. |
| M1-2 | Automatic interception/cancel ordering | Unit + integration | Brave | PASS | Browser cancelled only after `accepted`. |
| M1-3 | Redirected ISO and ranges | Integration | Brave | PASS | Redirected ISO was byte-for-byte equal to fixture; fixture SHA-256 `b850dda97b2c00055ee10ba703ce231427056ed16bb25714e3f2e2021fab6081`. |
| M1-4 | Content-Disposition/unknown length | Integration | Brave | PASS | Both completed through GrabBit. |
| M1-5 | Duplicate prevention | Unit + integration | Brave | PASS | Second idempotent request returned `duplicate`. |
| M1-6 | Disabled, Ask, exclusions | Unit + integration | Brave | PASS | Browser fallback and both Ask choices verified. |
| M1-7 | Native host unavailable | Integration | Brave | PASS | `app_unavailable`; browser download continued. |
| M1-8 | Cancellation failure | Integration | Brave | PASS | Recorded `duplicate_risk`; browser was not falsely reported cancelled. |
| M1-9 | Chrome automation | Attempted | Chrome | BLOCKED | Chrome 156 logs that command-line unpacked-extension controls are disallowed; no unsupported profile modification used. |
| M1-10 | ZIP and mid-transfer server interruption | No | No | NOT RUN | ISO, attachment, binary, redirect and unknown-length cases ran; interruption/ZIP remain manual. |
| M2-1 | Detection, placement, replacement, duplicate removal | Unit + one controlled-page run | Brave | PASS | One packaged-content-script run verified 10 px right/48 px bottom placement, tooltip, replacement/resize/fullscreen events, and removal. Six repeatable unit tests also pass. |
| M2-2 | Native media handoff/dialog opening | Native handoff + UI test | Brave | PASS | Real `INSPECT_MEDIA` returned `accepted`; focused UI test verified dialog opening. |
| M2-3 | Actual provider formats/audio/resolution | Simulated UI only | No | BLOCKED | UI selection passed with fixture formats, but a real yt-dlp result was not driven through the desktop WebView. |
| M2-4 | FFmpeg/FFprobe processing | Real local fixtures | N/A | PASS | Two tests verified the media matrix plus progress and child-process cancellation. |
| M2-5 | YouTube provider | No | No | NOT RUN | Requires permitted user media and interactive confirmation. |
| M3-1 | Close button hides window | Lifecycle test | No | BLOCKED | macOS Accessibility could not retain the window long enough to click its close control reliably. |
| M3-2 | Hidden process/IPC continuity | Lifecycle + process observation | N/A | PASS | Process and socket remained available with no visible windows. |
| M3-3 | Restore and single instance | Unit + actual second launch | N/A | PASS | Second executable exited normally; existing process restored/focused one window. |
| M3-4 | Explicit controlled Quit | Unit + actual release process | N/A | PASS | Application-menu Quit exited once and, after the fix, removed the IPC endpoint. |
| M3-5 | Tray menu actions | Unit/policy | No | BLOCKED | Tray Open/Quit could not be selected reliably through macOS Accessibility automation. |
| M3-6 | Download continues while hidden/cross-feature workflow | No | No | NOT RUN | Requires the remaining close/tray interaction. |
| M3-7 | Persisted jobs after restart | Connection restart only | Brave | NOT RUN | Desktop restart reconnected (`pong`), but job rows/progress were not asserted after restart. |
| M3-8 | Launch at login preference | Compile/typecheck | No | NOT RUN | Disabled by default; enabling/disabling OS registration remains manual. |

## Tests executed

- Extension: 4 files, 16 tests passed; extension typecheck and build passed.
- Focused frontend: 2 media-dialog tests passed; frontend typecheck passed.
- Rust: 7 lifecycle, 2 IPC cleanup, 3 browser registration, 1 durable finalization, and 2 FFmpeg/FFprobe tests passed.
- Desktop Clippy with warnings denied and `cargo check -p grabbit-desktop` passed.
- Real Brave run passed durable capture, automatic cancellation, ISO redirect, attachment, unknown length, Ask/off/exclusion modes, cancellation failure, unavailable fallback, browser restart, and desktop restart.

## Defect found and fixed

Normal macOS Quit could reach `RunEvent::Exit` without `ExitRequested`. Consequently `DownloadService::shutdown` could be skipped and `browser-ipc.sock` remained, while diagnostics treated the stale path as available. Shutdown now executes exactly once for either exit event, explicitly removes the Unix socket/Windows capability file, and Unix readiness requires a successful connection. The fresh release process was retested: process exited and the socket path was removed.

## Remaining manual reproduction

1. Load the stable packaged extension in Chrome manually, reload it, start GrabBit, and click ZIP and interrupted-download fixtures; verify browser fallback and SHA-256.
2. Start a throttled download, click the window close control, confirm the process/tray and progress continue, capture a second browser download, restore through **Open GrabBit**, then choose **Quit GrabBit**.
3. Toggle **Launch GrabBit when I sign in**, sign out/in, verify one instance, then disable it.
4. On a video the user owns or may download, verify YouTube/player placement and actual yt-dlp formats without DRM or access-control bypass.
