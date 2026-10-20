# M4 Core research: HW render, required env calls, BIOS, assets

Sources are primary (GitHub, pinned to a commit) unless marked otherwise. Installed nightlies
(`.testdata/data/cores/`) were checked read-only with `strings -n 6 <core.so> | grep -i <prefix>_`
to confirm option-key prefixes match the fetched source.

Pinned commits (`master`/`main` tip, fetched 2026-10-20):
- Beetle PSX HW: `libretro/beetle-psx-libretro@9bcc75d0a3e8310a273afb8fac1409224ff42c29`
- Mupen64Plus-Next: `libretro/mupen64plus-libretro-nx@6752836de8b224febfd5708444755b77712ac939`
- melonDS DS: `JesseTG/melonds-ds@f394adbacb5722ee97c1b37c8064da9a25818310`
- PPSSPP: `hrydgard/ppsspp@cf85e91e621b9412d4ba8d189bb0e1965a73dd54` (libretro backend lives in `libretro/`)

## Summary table

| Core | HW context requested | Depth/stencil | Shared ctx | Falls back to software? | GET_VARIABLE needed for HW render? | BIOS required | Assets needed |
|---|---|---|---|---|---|---|---|
| Beetle PSX HW | Auto: `GET_PREFERRED_HW_RENDER`→ `OPENGL` (compat) or `OPENGL_CORE` 3.3; retries the other flavour once if refused | depth=true, stencil=false | No (`cache_context=false`) | Yes, always lands on software if both HW attempts fail | **Yes** — if `GET_VARIABLE` fails for `beetle_psx_hw_renderer`, the core never calls `SET_HW_RENDER` at all | Optional — falls back to a bundled OpenBIOS HLE image with a log warning | None required; optional HD texture packs |
| Mupen64Plus-Next | Depends on RDP plugin: GLideN64 (default) → OpenGL (version negotiated inside the GLideN64 submodule, not traced here); ParallelRDP → `RETRO_HW_CONTEXT_VULKAN` 1.1 + `SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE`; Angrylion → no HW render | Not confirmed for GLideN64 path | Not observed | Angrylion is pure software; GLideN64/ParallelRDP failure behaviour not traced | No — missing `GET_VARIABLE` for `mupen64plus-rdp-plugin`/`-rsp-plugin` falls back to GLideN64 + HLE, still hardware-accelerated | None — N64 has no separate BIOS file | None required |
| melonDS DS | Always `RETRO_HW_CONTEXT_OPENGL_CORE`; 3.2 by default, 4.3 if the compute renderer is selected. No `GET_PREFERRED_HW_RENDER` call | depth=true, stencil=false | No (`cache_context=false`) | Yes — `RenderStateWrapper` catches `opengl_exception` and calls `FallBackToSoftware` | Not for the GL context itself, but **yes for FR-DS-1–3** (see below) | Optional for DS (FreeBIOS + generated firmware auto-substitute); DSi mode strictly requires real DSi BIOS7i/9i + firmware + NAND | None |
| PPSSPP | `ppsspp_backend` picks the context; if `GET_VARIABLE` fails, falls back via `GET_PREFERRED_HW_RENDER`/DUMMY and tries, in order, GL Core 3.1 → GL/GLES3 → Vulkan → D3D11 (Windows) → software | depth=true (ctor), stencil not set | Not observed | Yes — `LibretroSoftwareContext` is the unconditional last resort | No — DUMMY auto-probe still tries GL Core first | None (PPSSPP has its own HLE firmware) | **Yes** — `<system>/PPSSPP/` from `https://buildbot.libretro.com/assets/system/PPSSPP.zip`; core logs a warning pointing at `docs.libretro.com/library/ppsspp` if missing |

The single biggest cross-core risk: **Beetle PSX HW only calls `SET_HW_RENDER` if `GET_VARIABLE`
succeeds** for its own `renderer` option. Game37's current "return false for `GET_VARIABLE`" policy
means Beetle PSX HW will silently stay in software rendering — OpenGL hardware rendering added in M4
will have no effect on this core unless Game37 starts answering at least this one key truthfully.

## Beetle PSX HW

**1. Hardware render** (source `rhi/rhi_lib_gl.c` @ pinned commit, lines ~6690–6775, and
`rhi/rhi_intf.c` lines 291–431): on a desktop (non-GLES) build the core calls
`RETRO_ENVIRONMENT_GET_PREFERRED_HW_RENDER`. If the frontend answers `RETRO_HW_CONTEXT_OPENGL`, it
requests `OPENGL` (compatibility, no version fields set). Otherwise it requests
`RETRO_HW_CONTEXT_OPENGL_CORE` 3.3. If the first `SET_HW_RENDER` call is refused, it retries once
with the other flavour before giving up. `hw_render.depth = true`, `stencil = false`,
`bottom_left_origin = true`, `cache_context = false` — no `SET_HW_SHARED_CONTEXT`. If both attempts
fail (or the `renderer` option isn't readable), `rhi_intf_open` falls through to
`rhi_soft_open`, which always succeeds — the core keeps running in software. **Recommendation for a
GL-only frontend: answer `GET_PREFERRED_HW_RENDER` with `RETRO_HW_CONTEXT_OPENGL_CORE` and accept
`SET_HW_RENDER` for `OPENGL_CORE` version ≥ 3.3 with a depth buffer.**

**2. Required env calls**: `GET_VARIABLE` must succeed for `beetle_psx_hw_renderer` (source
`libretro.c` lines 291–322) — if it fails, `software_selected = true` unconditionally and
`SET_HW_RENDER` is never attempted. A value of `"hardware"`, `"hardware_gl"`, or `"hardware_vk"` is
needed; `GET_LOG_INTERFACE` is used for diagnostics but its absence doesn't block loading (verified:
`log_cb` calls are wrapped, no load failure path depends on it).

**3. BIOS/firmware** (source `libretro.c` lines 596–740, 3344–3430): region BIOS files go directly in
the system directory: `scph5500.bin` (JP), `scph5501.bin` (NA, also accepts `scph5503.bin`/`scph7003.bin`),
`scph5502.bin` (EU, also `scph5552.bin`). All are optional in this build — if none is found, the core
loads a bundled OpenBIOS image (`deps/openbios/openbios.bin.h`) and logs a warning rather than refusing
to load. A real BIOS is still strongly recommended for accuracy and for games that rely on exact BIOS
timing/behaviour.

**4. Assets**: none required. HD texture replacement packs are optional (`HD_TEXTURE_CACHE.md`).

## Mupen64Plus-Next

**1. Hardware render**: the graphics backend is chosen by the `mupen64plus-rdp-plugin` option
(`libretro.c` lines ~920–940 at pinned commit): `gliden64` (default), `angrylion` (CPU software,
no HW render), or `parallel` (ParallelRDP). ParallelRDP's `SET_HW_RENDER` call is explicit in
`libretro.c` lines 1850–1867: `context_type = RETRO_HW_CONTEXT_VULKAN`, `version_major =
VK_MAKE_VERSION(1,1,0)`, plus `SET_HW_RENDER_CONTEXT_NEGOTIATION_INTERFACE` — **not usable by a
GL-only frontend**. GLideN64's own `SET_HW_RENDER` call lives in its submodule
(`custom/mupen64plus-video-GLideN64/`), which this pass did not fetch — **unverified**: by general
knowledge GLideN64 requests an OpenGL compatibility (or GLES2/3) context and negotiates its GL version
internally; this needs confirming against the submodule before relying on it. Angrylion needs no
hardware context at all.

**2. Required env calls**: `GET_VARIABLE` for `mupen64plus-rdp-plugin`/`mupen64plus-rsp-plugin` is read
but not required — the fallback when it's missing is `RDP_PLUGIN_GLIDEN64` (`libretro.c` line ~940),
so the core still attempts hardware rendering with Game37's current env policy. `GET_RUMBLE_INTERFACE`
is referenced for the Rumble Pak; absence likely just disables rumble (not confirmed to block loading).

**3. BIOS/firmware**: none. The N64 has no separate BIOS chip; the PIF boot ROM is emulated/HLE'd
inside the core. Nothing goes in the system directory for a normal game to boot.

**4. Assets**: none required for GLideN64/Angrylion. ParallelRDP is not applicable to a GL-only
frontend.

## melonDS DS

**1. Hardware render** (source `src/libretro/render/opengl.cpp` lines 140–210, pinned commit): always
requests `RETRO_HW_CONTEXT_OPENGL_CORE`; version is `LEGACY_GL_VERSION` = **3.2** normally, or
`COMPUTE_GL_VERSION` = **4.3** if the compute-based renderer is selected (`HAVE_COMPUTE_RENDERER` build
flag + `RenderMode::Compute`). No `GET_PREFERRED_HW_RENDER` call — it asks for Core unconditionally.
`depth = true`, `stencil = false`, `bottom_left_origin = true`, `cache_context = false`; no
`SET_HW_SHARED_CONTEXT`. If `SET_HW_RENDER` (`retro::set_hw_render`) fails, the constructor throws
`opengl_not_initialized_exception`; `render/render.cpp`'s `RenderStateWrapper` catches that
(and `opengl_exception` generally, line ~249) and calls `FallBackToSoftware` — **confirmed graceful
fallback to the software renderer.**

**2. Required env calls**: none block loading outright. `GET_VARIABLE` failing just means every core
option takes its compiled-in default (confirmed in `environment.cpp`: a failed
`RETRO_ENVIRONMENT_GET_VARIABLE` makes `retro::get_variable()` return an empty `string_view`, and
`config.cpp`'s `ParseX(value)` then leaves the config field at its prior/default value). This matters
a great deal for FR-DS-1–3 — see below.

**3. BIOS/firmware** (source `src/libretro/config/console.cpp` lines 334–420, 488–523): for plain DS
content, native `bios7.bin`/`bios9.bin` plus `firmware.bin` are optional — if missing, melonDS DS logs
a warning and substitutes the built-in **FreeBIOS** (`melonDS::FreeBIOSGetNtrArm7/9`) and a generated
firmware. `SysfileMode` = Native requires all of them for a bit-accurate boot; Direct Boot mode is
forced if native boot isn't possible. **DSi mode has no such fallback**: real `dsi_bios7.bin`,
`dsi_bios9.bin`, a real DSi firmware, and a NAND image are mandatory — `LoadDSiBios`/firmware loading
throws (`dsi_missing_bios_exception`, `dsi_no_firmware_found_exception`) if any is absent.

**4. Assets**: none. (The two bundled `.ttf` fonts and error-screen PNGs under `src/libretro/assets/`
ship inside the core binary, not as system-directory files.)

## PPSSPP

**1. Hardware render** (source `libretro/LibretroGraphicsContext.cpp` lines 25–40, 94–158): `ppsspp_backend`
picks the context explicitly (`opengl`→`RETRO_HW_CONTEXT_OPENGL`, `vulkan`→`VULKAN`, `d3d11`→`D3D11`,
`none`→no HW render). If `GET_VARIABLE` fails, `backend` stays `RETRO_HW_CONTEXT_DUMMY`, and
`CreateGraphicsContext` calls `GET_PREFERRED_HW_RENDER` (defaulting to `DUMMY` if that also fails) and
then tries, **in this fixed order**, until one succeeds: GL Core 3.1 (`LibretroGLCoreContext`, source
`LibretroGLCoreContext.h` line 10: `RETRO_HW_CONTEXT_OPENGL_CORE, 3, 1`) → GL/GLES3 compat → Vulkan →
D3D11 (Windows only) → `LibretroSoftwareContext`, which never fails. `hw_render_.depth = true` always;
`stencil` is left at its zero-initialised default (false). No `SET_HW_SHARED_CONTEXT` observed.

**2. Required env calls**: none observed to block loading — the DUMMY auto-probe above means PPSSPP
tries hardware rendering even with Game37's current "answer false to everything" policy, unlike Beetle
PSX HW. `ppsspp_memstick_inserted` and similar are read but default sensibly if absent.

**3. BIOS/firmware**: none — PPSSPP has its own HLE for the PSP's boot ROM; no file goes in the system
directory for this.

**4. Assets**: **required** — `<system>/PPSSPP/` populated from
`https://buildbot.libretro.com/assets/system/PPSSPP.zip` (fonts, generated UI atlas, flash0 substitute
files). The core logs a warning pointing at `docs.libretro.com/library/ppsspp` when these are missing
(confirmed in `libretro.cpp` around line 1228–1260); without them PPSSPP still boots but font
rendering and some UI/HLE modules are degraded or missing.

## melonDS DS layout and touch: recommendation for FR-DS-1–3

Verified in `src/libretro/config/definitions/screen.hpp` and `constants.hpp` (exact key/value
strings) and `src/libretro/input/{pointer,joypad}.cpp` (input handling), all at the pinned commit.

**Core options to set** (as `GET_VARIABLE` responses — Game37 must start answering these truthfully;
the compiled-in defaults do *not* match FR-DS-1):

| Key | Value | Why |
|---|---|---|
| `melonds_number_of_screen_layouts` | `"2"` | Matches the default, but must be set explicitly if Game37 drives the other keys — keeps the R3 cycle to exactly two layouts. |
| `melonds_screen_layout1` | `"hybrid-top"` | Top screen large, bottom small — satisfies FR-DS-1. |
| `melonds_screen_layout2` | `"hybrid-bottom"` | Bottom screen large, top small — the other half of the R3 swap (FR-DS-2). |
| `melonds_hybrid_small_screen` | `"one"` | "Show Opposite Screen": renders exactly one large + one small (the mockup's layout). Default is `"both"`, which renders the large screen *plus both* DS screens small — not what the mockup shows. |
| `melonds_touch_mode` | `"auto"` | Accepts both mouse/pointer clicks and the joystick cursor, switching to whichever was used most recently — covers FR-DS-3's "mouse clicks, and a right-stick cursor" in one setting. |
| `melonds_show_cursor` | `"always"` (or leave the default `"timeout"`) | A visible cursor is needed to aim the right-stick pointer before tapping; `"always"` is the most predictable for a controller-first UI. |

Compiled-in defaults if these are *not* set (confirmed in `screen.hpp`): `screen_layout1 = "top-bottom"`,
`screen_layout2 = "left-right"`, `number_of_screen_layouts = "2"`, `hybrid_small_screen = "both"`,
`touch_mode = "auto"`, `show_cursor = "timeout"` (3s). None of the default two layouts is a
large+small hybrid, so **without Game37 driving these options, FR-DS-1 is not met** — this is the
most important finding in this document for M4.

**Inputs**: melonDS DS has its **own** binding for swapping screens — `RETRO_DEVICE_ID_JOYPAD_R3`
calls `ScreenLayoutData::NextLayout()` directly in `JoypadState::Apply` (`joypad.cpp`). The front end
does **not** need `GET_VARIABLE_UPDATE`/runtime option changes for this; it only needs to pass the
joypad bitmask through, which Game37 already does, and to have configured exactly two hybrid layouts
as above so R3 toggles between "top large" and "bottom large" — matching the mockup's R3 hint exactly.
Touch is read via `RETRO_DEVICE_POINTER` (`PointerState::Update`, using `poll.PointerPosition`) when
`touch_mode` is `Pointer` or `Auto`; libretro pointer coordinates are normalised 0–32767 relative to
the *entire* reported video frame, which for a hybrid layout is the small-screen sub-rectangle melonDS
DS itself lays out (the core does its own hit-testing against screen position, so the front end just
forwards raw pointer coordinates — no DS-screen-relative transform needed on Game37's side). In
`Joystick`/`Auto` mode, `RETRO_DEVICE_ID_JOYPAD_R2` is the tap button and the right stick drives
`AnalogCursorDirection` (`JoypadState::Update`) — this matches the mockup's "R2: Tap with cursor
(proposed)" exactly. melonDS DS draws its own on-screen cursor for joystick mode when `show_cursor` is
anything but `"never"` — the front end does not need to render one itself. `L2` speeds up cursor
movement while held; `L3` toggles the microphone, not touch-related.

**Framebuffer**: hybrid layouts render all screens (large + small, or large + both smalls) into a
single packed OpenGL/software framebuffer sized for that layout — vertex counts differ (12 vs 18,
`GetVertexCount` in `opengl.cpp`) but it is still one texture per frame. The output size is fixed per
layout choice and reported through `retro_get_system_av_info`/geometry at startup; switching between
the two configured layouts via R3 does **not** currently appear to trigger a runtime
`SET_GEOMETRY` in the files inspected (not exhaustively confirmed — `screenlayout`/geometry-update call
sites were not fetched in this pass) — **unverified; treat the two hybrid layouts as same-geometry
(same screen, same ratio) to avoid depending on live `SET_GEOMETRY` until this is checked against the
running core.**

**Conclusion**: melonDS DS's own hybrid layout, configured as above, produces exactly the mockup's
"one large screen + one small screen" composition — **Game37 does not need to compose the two DS
screens itself** from a raw top/bottom framebuffer. The only gap is that Game37 must start answering
`GET_VARIABLE` (at minimum for these six keys) instead of unconditionally returning false, or melonDS
DS will run with its default Top/Bottom + Left/Right cycle instead.
