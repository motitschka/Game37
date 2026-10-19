# External Emulators: launching, fullscreen, and exit behaviour

Research for the seven Standalone Emulators used by External Systems. All seven are installed from
Flathub on this machine (versions in the task table). Game37 launches each with
`flatpak run --filesystem=<Games Root>:ro <id> <args…>`, waits for the process to exit, and records
that wall time as Playtime.

## Summary table

| Emulator | Launch args (`{path}`) | Path for folder Games | Fullscreen | Exits when the Game stops? | Notes |
|---|---|---|---|---|---|
| Dolphin | `-b -e {path}` | n/a (single file) | No dedicated flag; `-C Dolphin.Display.Fullscreen=True` (community-reported) or GUI "Start in Fullscreen" | **Yes** — `-b`/`--batch` | Needs `--exec`/`-e` or `--nand_title`/`-n` alongside `-b` |
| Cemu | `-g {path}` [`-f`] | `code/<name>.rpx` (or the `.wua`/`.wud`/`.wux` file) | `-f`/`--fullscreen` | **Effectively yes** — but only because there is no "stop and return to list"; quitting the game *is* quitting Cemu | If a game returns itself to the Wii U menu without the player quitting, Cemu's window can stay open blank |
| Azahar | `{path}` [`-f`] | n/a (single file) | `-f`/`--fullscreen` | **No** — `Emulation ▸ Stop` returns to the game list, process keeps running | No CLI flag found for auto-exit |
| Ryubing (Ryujinx) | `{path} --fullscreen` | n/a (single file) | `--fullscreen` | **No** — closing/ESC leaves the process running (reported as leaving an invisible process behind in some versions) | First run needs `prod.keys` placed manually; first invocation also writes a default `Config.json` |
| PCSX2 | `-batch -fullscreen {path}` | n/a (single file) | `-fullscreen` | **Yes** — `-batch` ("exits after shutting down"); `-nogui` implies `-batch` | Flatpak needs the `--filesystem` grant (no home access by default) |
| RPCS3 | `--no-gui {path}` [`--fullscreen`] | `PS3_GAME/USRDIR/EBOOT.BIN` (disc dump) or `USRDIR/EBOOT.BIN` (extracted) | `--fullscreen`, "Only used when no-gui is set" | **Yes** (by design intent of `--no-gui`, used this way by frontends) | Needs firmware installed via the GUI once (`PS3UPDAT.PUP`); enforces a single instance via a lock file that can get stuck after a crash |
| shadPS4 | `-g {path} --fullscreen true` | `eboot.bin` | `--fullscreen true` (undocumented in `--help`, confirmed by maintainers/users) | **No** — the Qt window intentionally stays open after the game window closes | Needs firmware modules in `sys_modules`; fullscreen has a history of Linux freeze bugs (may or may not affect 0.19.0) |

Confidence key used below: **verified locally** (ran the command on this machine), **from docs at
<link>** (an official doc/wiki/source), **from source at <link>** (pinned GitHub/GitLab file or
commit), or **from community sources** (forums/issue trackers — lower confidence, flagged as such).

## Dolphin (org.DolphinEmu.dolphin-emu, 2609a)

**Verified locally** — `flatpak run org.DolphinEmu.dolphin-emu --help`:
```
-e <file>, --exec=<file>        Load the specified file
-b, --batch        Run Dolphin without the user interface (Requires --exec or --nand-title)
-C <System>.<Section>.<Key>=<Value>, --config=...   Set a configuration option
```
No `--fullscreen`/`-f` flag exists in this build (older Dolphin releases had one; it was removed).
Direct launch: `-b -e {path}`.

Fullscreen: no CLI flag. **From community sources** (LaunchBox/forum threads, not Dolphin's own
docs): pass `-C Dolphin.Display.Fullscreen=True` via the generic `-C` option, equivalent to
checking "Start in Fullscreen" under Graphics ▸ General. I could not find this key name in Dolphin's
own documentation or source in the time available — treat it as **unverified** and test it, or set
"Start in Fullscreen" once in Dolphin's own config instead.

Exit behaviour: `--batch` is documented by Dolphin itself as running "without the user interface,"
and **community sources** (LaunchBox/GameEx forums) consistently describe it as exiting once the
loaded title stops, which is the standard use of this flag for frontends. Not contradicted anywhere
I found.

## Cemu (info.cemu.Cemu, 2.6)

**Verified locally** — `flatpak run info.cemu.Cemu --help`:
```
-g [ --game ] arg       Path of game to launch
-f [ --fullscreen ] [=arg(=1)]   Launch games in fullscreen mode
-m [ --mlc ] arg        Custom mlc folder location
-a [ --account ] arg    Persistent id of account
```
Folder-game path: **from community sources** (gbatemp.net command-line thread) — `-g` takes the
direct path to the `.rpx` inside `code/`, e.g. `-g ".../GameName/code/game.rpx"`, not the folder
itself. A `.wua`/`.wud`/`.wux` package is passed as that single file's path instead.

Exit behaviour: Cemu has no batch/no-gui flag. More importantly, Cemu currently has **no way to stop
emulation and return to a game list at all** — confirmed by the maintainer closing
[cemu-project/Cemu#1794](https://github.com/cemu-project/Cemu/issues/1794) "not planned": *"Currently,
once a game has been started in Cemu, it is not possible to stop the emulation... the only available
option is to fully exit and restart Cemu."* So for a game launched via `-g`, the player's only
"stop" action (closing the window / File ▸ Exit) is the same action as exiting the process — Playtime
measurement should be accurate in the normal case. The open request
[cemu-project/Cemu#1518](https://github.com/cemu-project/Cemu/issues/1518) ("exit program when game
closes") implies the edge case: if the *game itself* returns to a simulated system menu rather than
the player quitting, Cemu's window can be left open and blank, with no flag to auto-close it.

## Azahar (org.azahar_emu.Azahar, 2126.2)

**Verified locally** — `flatpak run org.azahar_emu.Azahar --help`:
```
Usage: azahar [options] <file path>
-f, --fullscreen     Start in fullscreen mode
-w, --windowed       Start in windowed mode
```
Direct launch is a bare positional `{path}` argument. No folder-game case (3DS files are single
files).

Exit behaviour: **from community sources** (forum threads, deepwiki overview of Azahar's game-list
module) — `Emulation ▸ Stop` returns to Azahar's own game-list window; the process keeps running.
No CLI batch/no-gui equivalent was found in `--help`, in Azahar's docs, or in a search for one.
Also note [azahar-emu/azahar#677](https://github.com/azahar-emu/azahar/issues/677): on some systems
the window can close while the underlying process does not exit even on a full quit — a possible
extra source of inflated Playtime if it reproduces here; not something `--batch`-style flags fix
since none exist.

## Ryubing / Ryujinx (io.github.ryubing.Ryujinx, 1.3.3)

`flatpak run io.github.ryubing.Ryujinx --help` opened the GUI and printed only startup/config log
lines (no usage text) before being killed by `timeout`; it is not a `--help`-aware CLI in this
build. Falling back to the project's own docs.

**From docs at** [Ryubing/docs](https://github.com/Ryubing/docs) (`docs/info/faq-&-troubleshooting.md`,
section "Launch in Fullscreen"): *"If you launch Ryujinx from the commandline, add a `--fullscreen`
option."* The same doc shows `-r <folderpath>` for a custom portable data directory, used as a bare
flag alongside a positional game path. No `-g`/`--game` flag — **from community sources**
(LaunchBox forum thread): a plain positional path is correct, and using `-g` is a user mistake (it
is not a recognised option).

Exit behaviour: **from source/issue tracker** —
[Ryubing/Issues#415](https://github.com/Ryubing/Issues/issues/415), filed against this emulator
family, confirms that exiting a game (including via the fullscreen/ESC path) can close the visible
window while "the main Ryujinx process continues running in the background." Even setting that bug
aside, the normal design (shared with Yuzu-family emulators) is to return to the in-process game
list on stop, not to exit — there is no batch/no-gui flag anywhere in the docs I found. Treat this as
**No, does not exit**, with a known extra failure mode of a fully headless leftover process.

Gotchas: first run needs `prod.keys` copied into the data folder manually (**from docs**,
`setup-guide.md`) — there's no firmware/keys prompt dialog, just a persistent warning in the main
window until the file is placed; a batch/headless launch would have no way to surface that warning.
The very first invocation of the Flatpak (confirmed **verified locally**, our `--help` run) writes a
default `Config.json` and other files before doing anything else, including before showing any UI.

## PCSX2 (net.pcsx2.PCSX2, v2.8.2)

`timeout 10 flatpak run net.pcsx2.PCSX2 -h` opened the Qt GUI instead of printing help and had to be
killed after the timeout (PID confirmed and terminated). Falling back to PCSX2's own docs.

**From docs at** [pcsx2.net/docs/advanced/cli](https://pcsx2.net/docs/advanced/cli/) (mirrored at
[PCSX2/pcsx2-net-www](https://github.com/PCSX2/pcsx2-net-www/blob/main/docs/advanced/cli.md)):
```
-batch       Enables batch mode (exits after shutting down)
-nogui       Hides main window while running (implies batch mode)
-fullscreen  Enter fullscreen immediately
-elf <file>  Override the boot ELF with the specified filename
```
Usage line: `pcsx2-qt [parameters] [--] [boot filename]` — the game path is a trailing positional
argument. Direct launch: `-batch -fullscreen {path}`.

Exit behaviour: **Yes**, documented directly — `-batch` "exits after shutting down." `-nogui`
implies it, so either is sufficient; Game37 can use `-batch -fullscreen {path}` without `-nogui` if a
window is wanted, or add `-nogui` for a fully headless run.

## RPCS3 (net.rpcs3.RPCS3, 0.0.43)

**Verified locally** — `flatpak run net.rpcs3.RPCS3 --help` (full output captured):
```
--no-gui             Run RPCS3 without its GUI.
--fullscreen         Run games in fullscreen mode. Only used when no-gui is set.
--installfw <path>   Forces the emulator to install this firmware file.
```
Usage: `rpcs3 [options] (S)ELF [Args...]` — positional path. Direct launch:
`--no-gui {path}`, add `--fullscreen` for fullscreen (only takes effect alongside `--no-gui`, per the
emulator's own `--help` text). Folder games: `PS3_GAME/USRDIR/EBOOT.BIN` for a disc dump, or
`USRDIR/EBOOT.BIN` under an extracted game (both per the given facts, confirmed compatible with the
"(S)ELF" positional argument).

Exit behaviour: `--no-gui` is specifically meant for frontend integration in this way, and this is
corroborated **from community sources** (RetroDECK/Batocera emulator guides, ArchWiki) that describe
exactly this combination for exit-after-game use. I did not find RPCS3's own docs stating the exit
behaviour in so many words, so this is **from community sources**, not a primary doc.

Gotchas: firmware (`PS3UPDAT.PUP`) must be installed once via the GUI (`File ▸ Install Firmware`) or
via `--installfw <path>` — **verified locally** from `--help`, this can be done from the CLI without
ever showing the GUI, so Game37's first-run flow could drive it headlessly if it has a PUP file.
RPCS3 enforces a single instance via a lock file; **from community sources**
([RPCS3/rpcs3#19317](https://github.com/RPCS3/rpcs3/issues/19317)) a stale lock left behind by a
non-clean shutdown (e.g. a hard kill) makes the next launch fail with "another instance is running" —
so Game37 must let `--no-gui` runs exit on their own and avoid `SIGKILL`.

## shadPS4 (net.shadps4.shadPS4, 0.19.0)

**Verified locally** — `flatpak run net.shadps4.shadPS4 --help`:
```
-g, --game <ID|path>   Specify game to launch.
-s, --show-gui         Show the GUI.
-i, --no-ipc           Disable IPC.
```
No `--fullscreen` is listed in this build's `--help`, but **from community sources** (shadPS4 forum,
corroborated by [shadps4-emu/shadPS4#1348](https://github.com/shadps4-emu/shadPS4/issues/1348),
which describes testing "a new fullscreen feature with an `-f` flag through the CLI") the flag is
`--fullscreen true` (also usable as `-f true`). Direct launch: `-g {path} --fullscreen true`, folder
games pass `eboot.bin` directly.

Exit behaviour: **No** — **from community sources** (shadPS4 issue discussion on the GUI/game-window
relationship) the Qt interface is deliberately kept open when the game window is closed, so a
subsequent launch doesn't have to restart the whole emulator; there is no `--no-gui`/batch
equivalent in `--help` or the README. Expect the flatpak process to outlive the played game.

Gotchas: needs firmware modules placed in `sys_modules` (not covered by the given facts, worth
flagging). Fullscreen crashed/froze the renderer on Linux in multiple historical issues (#1348,
#1888, #865) — all filed in 2024; I could not confirm from available sources whether 0.19.0 (current)
still reproduces this, so treat `--fullscreen true` on this machine as worth a manual smoke test
before relying on it.

## Question 5: does `flatpak run` block until the sandboxed app exits?

No single authoritative doc states the default blocking behaviour outright — the man page
(`flatpak-run(1)`) documents `--die-with-parent` ("kill the sandbox when the launching process
dies") but doesn't describe the normal case. **Verified locally**: in every one of the seven test
invocations above, `flatpak run <id> --help` did not return control to the shell until the
underlying app itself exited — for Dolphin/Cemu/Azahar/RPCS3/shadPS4 that happened quickly (exit
code 0); for Ryujinx and PCSX2, which opened a GUI instead of printing help, the wrapper command was
still running when `timeout 10` killed it (exit code 124), i.e. `flatpak run` was still blocked on
the app, not returning early. This is consistent with `flatpak run` behaving like a normal blocking
subprocess launch for all seven.

The known exception, **from docs/issue discussion** at
[flatpak/flatpak#5503](https://github.com/flatpak/flatpak/issues/5503): apps that use D-Bus
activation/single-instance patterns (GApplication-style) can have a second `flatpak run` just signal
the already-running instance over D-Bus and return almost immediately, without waiting for that
instance's lifetime at all. None of the seven emulators appeared to do this in the quick local test,
but Game37 should still make sure no other instance of the target emulator is already running before
launch — RPCS3's own single-instance lock (above) is one concrete case where a stray leftover
process could interact badly with a fresh `flatpak run`.

## Question 6: GNOME/Wayland — can Game37 bring its own window back to front?

SDL3's Wayland backend does implement `SDL_RaiseWindow` using the `xdg-activation-v1` protocol
(**from source at** the SDL commits "wayland: Implement RaiseWindow with xdg-activation" and
"wayland: Refine window raising for new windows", libsdl-org/SDL). The SDL3 wiki itself
(`SDL3/SDL_RaiseWindow`) only says the result "is subject to desktop window manager policy,
particularly if raising the requested window would result in stealing focus from another
application" and doesn't name Wayland specifically.

`xdg-activation-v1` requires an activation **token**, normally minted from a recent user-input
serial in whichever surface is requesting the activation (**from docs**, the GNOME Discourse thread
"Cross-process window activation on Wayland"). Game37 calling `SDL_RaiseWindow` right after
`waitpid()` on the emulator process has no such input-backed token to attach — it isn't a response
to a click or key press, it's a timer/process-exit callback. GNOME Shell's documented
focus-stealing-prevention design treats an un-tokened activation request as exactly the case it's
built to deny: the practical result on GNOME/Wayland is that the window gets marked "ready"/flashes
in the overview or dash, rather than actually gaining focus and coming to the front. I could not find
a primary GNOME/Mutter source saying this in those exact words for this exact scenario (process exit
→ self-raise with no token) — this is a reasoned inference from the token requirement plus GNOME's
documented policy, not something tested locally, so treat it as **likely but unverified** behaviour
specifically for Game37's case.

Practical workaround, not requiring a cross-process token: a window **restoring itself** after being
minimised is a self-directed action, not a cross-process focus steal, and is reliably permitted.
Recommended approach: Game37 calls `SDL_MinimizeWindow()` on its own window right before invoking
`flatpak run` for the emulator, then after the emulator process exits, calls
`SDL_RestoreWindow()` followed by `SDL_RaiseWindow()`. Restoring one's own previously-iconified
window sidesteps the activation-token requirement entirely, so this should work reliably on
GNOME/Wayland even though a bare `SDL_RaiseWindow()` call on an already-visible-but-background
window might not.
