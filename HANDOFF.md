# HANDOFF — VItoriaBr build on D:

## Request
"Based on this repository (`C:\Users\Administrator\Downloads\VItoriaBr-main (1)`), recompile the game so it runs on PC; do the build on drive D:\".

## What the repository actually is (measured, from README and source)
VItoriaBr is a Rust runtime (fork of Marvisak/VIctory) that runs **an Xbox Series X|S title from a dump the user supplies**.
It ships no game files. The build output is the runtime `victory.exe`, not a standalone game executable.
README status (author's claim, not verified here): boot/menus/video/saves work; the 3D world is "not playable yet".

## Layout on D:
| Path | Contents |
|------|----------|
| `C:\Games\VItoriaBr\src` | Copy of the repo (169 files) |
| `C:\Games\VItoriaBr\toolchain\rustup`, `\cargo` | Rust 1.98.1 x86_64-pc-windows-msvc (pinned by `rust-toolchain.toml`) |
| `C:\Games\VItoriaBr\toolchain\BuildTools` | VS 2022 Build Tools, MSVC 14.44.35207 + Windows 11 SDK 10.0.26100 |
| `C:\Games\VItoriaBr\target` | Cargo output (`CARGO_TARGET_DIR`) |
| `C:\Games\VItoriaBr\target\release\victory.exe` | Release build (7,763,456 bytes) |
| `C:\Games\VItoriaBr\victory.toml` | Config: game/cache/data dirs under `C:\Games\VItoriaBr\victory-data\` |
| `C:\Games\VItoriaBr\env.cmd` | Loads MSVC + Rust environment, cd's into src |
| `C:\Games\VItoriaBr\run.cmd` | Runs `victory.exe run <exe> --execute` |

Note: the Windows SDK and VS Installer shared components are partly installed on C: (the VS installer puts the Windows Kits under `C:\Program Files (x86)\Windows Kits` and does not let you move them).

## Build and tests (commands and exit codes)
- `cmd /c "call C:\Games\VItoriaBr\env.cmd && cargo build --release -p victory --locked"` → **exit 0** (1m15s)
- `victory.exe --help`, `victory.exe config` → **exit 0**, config resolves to the D: paths
- `cargo test --workspace --locked --no-fail-fast` → initially **exit 101** (533 passed, 5 failed); after the fixes below → **exit 0** (`C:\Games\VItoriaBr\logs\cargo_test_all.log`): 506 passed, 0 failed, 8 ignored (the final log is authoritative for this run).
  - Fixed: the three `victory-shader` cases declaring `WaveSize(32)` failed with E_INVALIDARG on the RX 580 (`WaveLaneCountMin=Max=64`); `Host::runs_wave()` now queries `D3D12_FEATURE_DATA_D3D12_OPTIONS1` and the tests skip with an explicit `waves of 32 lanes unsupported; skipped` instead of panicking on a device limit. Other failures still panic normally.
  - Fixed: `xtask policy::tests::repository_passes_policy` needed git on `PATH`; the portable binary already exists at `C:\Games\VItoriaBr\toolchain\git\cmd\git.exe` (`C:\Games\VItoriaBr\test-policy.cmd`). With no `.git` the test uses its WalkDir fallback; no repository was created.
  - The old "5 failed" count named only four tests; no fifth failure was present in any saved log.

## Host GPU measured (scratch D3D12 probe)
AMD Radeon RX 580 2048SP, driver 31.0.21925.1001: feature level 12_0, **SM 6.6 yes, Mesh Shader tier 0 (NOT supported)**, wave 64.
In the code, `crates/victory-gpu/src/pipeline/host.rs:359` refuses every pipeline that uses a mesh shader when the host has none
→ README counts 8,765 mesh shaders; on this GPU those draws will be dropped. Expect menus/video at best and an incomplete or black 3D world. Not verified in-game.

## Game dump (provided by user: `C:\Users\Administrator\Downloads\GTA 6`)
- `MicrosoftGame.Config`: Grand Theft Auto VI, TitleId `7952987A`, version `1.0.2.1` (same build as the README).
- Copied to `C:\Games\VItoriaBr\victory-data\game` with robocopy: 109 files, 208.617 GB, 0 failures (`C:\Games\VItoriaBr\copy_game.log`).
- `game_scarlett_master.exe` SHA-256 `E95020B2FA4A721DB3DF59FDECF75621730AC559D8835252F6C3B658D5A08993` (identical on C: and D:). victory reports `build unknown` (its `KNOWN_BUILDS` list is empty).
- `victory.exe inspect` → exit 0. `victory.exe census` → exit 0 (995 imports total).
- `victory.exe run C:\Games\VItoriaBr\victory-data\game\game_scarlett_master.exe` (bind only, no `--execute`) → exit 0; exe + 4 title DLLs bound with **0 missing imports**; virtual drive G: = D: copy; saves under `C:\Games\VItoriaBr\victory-data\data\7952987A`.
- The original folder on C: is untouched (not deleted).

## Verified boot (2026-11-25, D: build)
`victory.exe run C:\Games\VItoriaBr\victory-data\game\game_scarlett_master.exe --execute` was run twice.
The first run was killed by the harness after 50 s (not a crash); the second was left running.

Evidence (`C:\Games\VItoriaBr\logs\run_execute2.err`, 195,966 bytes; `C:\Games\VItoriaBr\logs\game_screen.png`, 3,485,300 bytes):
- process 6908 still alive after 100 s, window title `Americas | VIctory 29 fps 34.4 ms/frame 6 draws/frame`
  (later `... 24 draws/frame`); window handle 6686302.
- image mapped; 4 title DLLs plus the exe bound with **0 missing imports**; virtual drives G:, T:, Z:.
- D3D12.X device `0x20004`, host wave 64, title window fitted from 3840x2160 to 1848x1040, 30 fps interval.
- shaders translated and PSOs made: first compute at frame 1080, vertex/pixel at frame 2979;
  pipeline library saved (`made=7 loaded=4`, 2482 bytes).
- `frame presented frame=1024 draws=1536 drawn=1536`, later `24 draws/frame`,
  `dropped 0 without a host pipeline`, GPU ~9.7 ms/frame.
- **0 ERROR, 0 panic, 0 crash, 0 access violation**; no `crashes.log` was produced.
- screenshot pixel-grid analysis shows a rendered, non-black scene with a text strip at the bottom
  (image could not be visually inspected by the agent that captured it).

Warnings that remain (not fatal):
- `rpcrt4.dll` refused by host policy; console instruction `Clwb` emulated (logged once).
- `slot=0x218` pipeline creation method not implemented: "the title goes without this pipeline"
  (repeated 31 times); draws still report 0 dropped.
- RX 580 has no mesh shader tier, so mesh-shader draws are expected to be dropped later;
  the README's "3D world not playable yet" was not disproved by this boot.

`C:\Games\VItoriaBr\capture.ps1` reproduces the run plus a screenshot (call with
`powershell.exe -NoProfile -ExecutionPolicy Bypass -File ...`; script execution policy is Restricted).

## Second repo: victory-amd (`C:\Users\Administrator\Downloads\victory-amd-victory-amd`)
- Fork of fxceless/victory-fork with AMD fixes; author tested on RX 7800 XT (has mesh shaders). Same mesh-shader refusal at `crates/victory-gpu/src/pipeline/host.rs:354`.
- Copied to `C:\Games\VItoriaBr\src-amd`, built to `C:\Games\VItoriaBr\target-amd\release\victory.exe`: `cargo build --release -p victory --locked` → exit 0.
- Tests `--no-fail-fast` → exit 101: 512 passed, 2 failed (`ds_min_max_f32...` E_INVALIDARG on RX 580; xtask policy needs git). The other two RX 580 failures of VItoriaBr do not exist in this fork.
- Bind on the D: dump → exit 0, 0 missing imports. Uses its own cache `C:\Games\VItoriaBr\victory-data\cache-amd`. Launcher `C:\Games\VItoriaBr\run-amd.cmd`.
- Three-way comparison with fxceless/victory-fork `main` (zip, latest commit 39731795d2 2026-11-25, in `C:\Games\VItoriaBr\upstream`): 101 files identical, 26 differ in all three, 12 only AMD changed, 8 only VItoriaBr changed, 22 where both forks agree and upstream moved on. Both forks lag upstream main.
- Not merged: no git history in either ZIP, so no common base; a blind merge of the 26 three-way-different files (lower.rs, descriptor/host.rs, host.rs, draw.rs...) is not verifiable on this GPU.

## Final state (2026-11-25)
- Tests: fixed and green — `C:\Games\VItoriaBr\test-all.cmd` → `C:\Games\VItoriaBr\logs\cargo_test_all.log`,
  23 suites, 506 passed, 0 failed, 8 ignored. Targeted scripts: `test-policy.cmd`, `test-shader-fix.cmd`.
- Release rebuilt after all fixes: `C:\Games\VItoriaBr\build-release.cmd` → exit 0,
  `target\release\victory.exe` 7,769,088 bytes, 2026-11-25 15:06:15.
- Final boot through `C:\Games\VItoriaBr\run.cmd` (which sets `VICTORY_CARD_BUFFERS_MIN_MB=1`):
  PID 20100, window `Americas | VIctory 30 fps 33.3 ms/frame`,
  `C:\Games\VItoriaBr\logs\run_intro_final.out` / `run_intro_final.err`, 0 panics, 0 CPU shadow compares,
  1 card copy of 174 MB. Earlier boot with the heap patch alone: PID 20932, 73 samples of
  `heap buffers 0.0 ms`, 26 lines at 29-31 fps.

Known limits, deliberately left alone:
- `slot=0x218` is an unimplemented method in the emulated D3D12 device vtable
  (`device.rs:359-399`, `f[67]` at `device.rs:441`). It returns `E_NOTIMPL` and nulls `out`.
  The file is byte-identical in `src`, `src-amd` and `upstream`; no copy has an implementation.
  It is distinct from `dropped ... without a host pipeline`. Binding it to `create_stream` would
  be unsafe: the description layouts differ. Only diagnostic decoding is advisable.
- RX 580 has no mesh shader tier; those draws are refused by `pipeline/host.rs:359`.
  Full 3D playability remains unverified (README: "not playable yet").
- The AMD fork boot was not compared on the dump; its release is still available at
  `C:\Games\VItoriaBr\target-amd\release\victory.exe`.

## Intro stutter investigation, four reference repos (2026-11-25, second round)
Cloned read-only references under `C:\Games\VItoriaBr\refs\`: `belobug`, `dexnom`, `fxceless`, `uiliannn`.
No game files or `.git` were added to `src`.

Diagnosis (evidence, not guesswork):
- The intro's ~2.5 s first-frame cost is present identically in the baseline and in every A/B run
  (`t≈9.7 s`, 2511/2582/2612 ms) — a one-time initialization cost, not the intermittent hitch.
- `fxceless/README.md:111-138` measures the real dips: they happen while the camera moves, with the
  card idle ~330 ms of a ~490 ms frame; the title waits for its own jobs, dominated by
  `CopyPageMappingsBatchX -> xmem::map_alias` (~850 page remaps in a streaming frame).
  Skipping redundant remaps was tried (none exist); batching pages cut remap time 140→79 ms but
  produced black/red faces and garbage draws, so it was reverted. **Not ported.**
- Our logs show no `dstorage` retry storm (`after a retry` = 0) and ~0 ms waiting for title memory
  to hold still, so `VICTORY_TEXTURE_NO_WAIT` / `VICTORY_DSTORAGE_RETRIES` would not help here.

Measured, safe improvements applied:
- `VICTORY_CARD_BUFFERS_MIN_MB=1` (already supported by `shadow.rs:154`): removes ~350 MB/frame of
  CPU compares (228 `shadow: shader writes` / `hash passes` log lines per frame in the baseline → 0),
  with no new warnings (host pipeline refused/does not translate/slot counts identical) and 6 min
  of monitoring at 25-30 fps. Added to `C:\Games\VItoriaBr\run.cmd`. GPU memory stays at 1 copy / 174 MB.
  Budget guards in `shadow.rs:211-218` still cap card copies at 512 MB and 80% of VRAM.
- Ported from `uiliannn` into `src\crates\victory-gpu\src\draw.rs::heap_buffers`: the full rescan
  is now every 64 passes (`VICTORY_HEAP_RESCAN_FRAMES` overrides, min 1), spans are cached between
  passes, and `device_removed()` is queried every 256 views instead of 64. The baseline logged
  `heap buffers 1.0 passes 226.4 ms` in the loaded scene; the fork reports 87-110 ms/frame saved.
  `VICTORY_UNWATCHED_FRAMES`, `HEAP_VIEWS_ELSEWHERE` and the removal diagnostics are unchanged.

Verification: `test-gpu-heap.cmd` → 112 passed after the change (the first run crashed with
`0xC0000005` only because the game was still running; with the game closed it passes);
`test-all.cmd` → exit 0, 23 suites, 506 passed, 0 failed, 8 ignored; `build-release.cmd` → exit 0.
Boot with the rebuilt binary: PID 20932, `Americas | VIctory 30 fps 33.3 ms/frame`, 0 panics.
Not ported: page-mapping batching (corrupts textures), `JOIN_PARTS` / `BACK_OWN_VIEW` /
`INDIRECT_GATHER` / `QUEUE2` / `ASYNC_SUBMIT` / `COPY_PLAIN` — they are larger merges, and their
measured gains are for a scene standing still, not the title's page-mapping dips.

## Can the 3D world run on the RX 580 2048SP? (checked 2026-11-25)

**No, not on that GPU.** Searched all 7 local trees (`src`, `src-amd`, `upstream`, `belobug`,
`dexnom`, `fxceless`, `uiliannn`): none has a mesh -> vertex/compute fallback, and no env var
(`VICTORY_NO_MESH`, `VICTORY_EMULATE_MESH`, `VICTORY_MESH_OFF`, ...) exists. Every tree uses the
same hard gate: `pipeline/host.rs:359` (`src`) refuses a PSO containing a mesh stage when
`Host::mesh_shaders()` is false. That check requires BOTH `MeshShaderTier != 0`
(`D3D12_FEATURE_D3D12_OPTIONS7`) and shader model 6.6. The RX 580 reports SM 6.6 but tier 0
(log: `host mesh shader support mesh=false`, then `no host pipeline: the host GPU has no mesh
shaders ... shaders=[mesh ..., pixel ...]`). `VICTORY_MESH_TEST=skip|scissor` is diagnostic only:
it drops or clips mesh draws, it does not translate them. Bypassing the gate would make
`CreatePipelineState` fail with E_INVALIDARG or reset the driver (TDR); ~8,762 of 8,765 title
shaders are mesh, so this is not a small missing feature.

**The only path that renders mesh 3D here is the software adapter (WARP), not the RX 580.**
Verified, not assumed:
- Built `C:\Games\VItoriaBr\target-diag\release\victory.exe` with `--features diagnostics`
  (`C:\Games\VItoriaBr\build-diag.cmd`).
- `VICTORY_D3D12_WARP=1` -> log `host device on the software adapter (WARP)`, and
  **0** "host GPU has no mesh shaders" refusals (baseline RX 580 logs: 1 each).
- `cargo test -p victory-gpu --features diagnostics --locked --
  a_mesh_pipeline_loads_from_the_library --nocapture` -> **ok** (not skipped): WARP reports a
  mesh tier and creates a mesh SM 6.6 PSO.
- WARP boot works but is very slow: 13 fps in menus, dropping to 3-4 fps, 1 fps deep in loading
  (1281 ms/frame, GPU time ~1260 ms). With scripted Enter presses the run reached the loading
  phase (`story000_restofworld.rpf`) but still no nonzero mesh dispatch before the test was
  stopped; a full software playthrough to the 3D world was not completed.
- Launcher: `C:\Games\VItoriaBr\run-warp.cmd <game exe> ["<seconds>:Enter,..."]`
  (uses the separate `victory-data\cache-warp` cache; do not use for performance measurements).

Conclusion for the **7 local trees above**: hardware 3D on the RX 580 would require writing a
mesh -> vertex/compute translator that does not exist there. WARP is a correctness check only.
This conclusion was later superseded by the external PRs in the next section.

## External mesh -> compute fallback found (2026-11-25, third round)

Web search found patches outside the seven local clones:
- `Marvisak/VIctory#961` / `greyfinch/VIctory#961`: `VICTORY_MESH_FALLBACK=1`, compute fallback.
- `greyfinch/VIctory#1648` / `Marvisak/VIctory#1647`: `VICTORY_MESH_EMULATION=1`, compute +
  unpack-vertex emulation; reports of very low FPS and no reliable end-to-end gameplay.
- `Marvisak/VIctory#2203` / branch `ElSeba0811/VIctory@codex/gtx-upstream-update`: ports the
  earlier GTX compute/vertex fallback to current upstream, with wave64 support (Polaris/RDNA1).
  Reports on RX 470 also mention `amdxc64.dll` crashes and many descriptor-heap warnings.
- References only: `StarsX/MSFallback`, `zortos293/Lichii#569`.

Chosen for testing: PR #2203, cloned read-only to `C:\Games\VItoriaBr\refs\gtxfallback` at commit
`9b770602b4386b6c672022fecf3ec4ea194f399a`. This is a **separate build**; `C:\Games\VItoriaBr\src`
and `target\release\victory.exe` were not changed. **Keep using `run.cmd` as the working
launcher until an experimental build proves stable.**

Artifacts:
- `C:\Games\VItoriaBr\build-mesh580.cmd` → `target-mesh580\diag\victory.exe` (7,913,984 bytes),
  built with `--features diagnostics`.
- `C:\Games\VItoriaBr\victory-mesh580.toml` and `victory-data\cache-mesh580` (separate cache).
- `C:\Games\VItoriaBr\run-mesh580.cmd`: `VICTORY_MESH_COMPUTE=1`,
  `VICTORY_MESH_WAVE64_DAG=1`, `VICTORY_MESH_INDIRECT=1`, `VICTORY_SNAPSHOT_POOL_MIB=128`,
  `VICTORY_D3D12_DRED=1` (DRED added after the first trial; the warm A/B still uses the old env).
- `C:\Games\VItoriaBr\run-mesh580-pool0.cmd`: same, but `VICTORY_SNAPSHOT_POOL_MIB=0` for A/B.
- `refs\gtxfallback\docs\mesh-fallback.md` in the clone documents the limits; notably 1M host
  descriptor views (2M exceeded this GPU's allocation limit), no general wave64 emulation,
  indirect scratch bounded to 4 MiB, and two known upstream shader-test failures.

First hardware trial (`logs\run_mesh580.err`, PID 19036):
- Reached the deep intro/world stream (`fbk_stry000_FBK_STRY000_INTRO`, frame 10357),
  **16** `experimental compute and vertex mesh pipeline` messages and **11**
  `experimental indirect compute and vertex mesh draw recorded` messages — the first proof that
  mesh draws can execute on this RX 580 (tier 0), with **0** "host GPU has no mesh shaders"
  refusals and **0** panics from the emulator.
- It was **not stable**: after ~14 minutes the title died with `0xc0000005` in
  `amdxc64.dll+0xdaa6f9`; the GPU frame had fallen to ~0.1-0.2 fps with 4,878-5,812 draws
  dropped/frame while thousands of pipelines were still being made. No `device removed` or
  `E_OUTOFMEMORY` was logged before the driver crash.
- 310,723 `title slot past the host heap` warnings: title descriptor indices above the 1M
  host heap are omitted, as documented by the branch; this is a correctness/visual limit, not
  proven to be the crash cause.
- Crash log: `victory-data\data\7952987A\temp\victory-20261125-154057-19036.crash.log`.
  Title memory was 2,177,602,432/2,305,818,624 physical and had 590 MB unused; no host panic.

A warm-cache A/B (`trial-mesh580.ps1`, tag `mesh580_warm`) **also crashed**, after 4m37s at
title frame 1923, before any fallback pipeline was created. Crash log:
`temp\victory-20261125-155145-20524.crash.log`. It is the same fault as the first run
(`0xc0000005` at `amdxc64.dll+0xdaa6f9`, same D3D12/victory backtrace), so it is a deterministic
driver fault around PSO creation, not just shader-compile overload or the indirect fallback.
The warm run had 241 `host pipeline refused` entries and only one mesh translation refusal.
DRED was added to the launcher after this run; it was not active in the failed process.

Upstream reports read after the first trial (PR #2203 comments, 2026-11-25):
- The same branch was fixed for invalid format-86 UAVs in `9b77060` (our clone already has it).
  On a GTX 1070 Ti it survived 163 s but then hit `E_OUTOFMEMORY` around 22 GiB process commit.
- L1DRE's patches and `VICTORY_SKIP_PIPELINES` for the BVH builders/ray-traced pass gave 22+
  minutes without device removal, but the world stayed black; only credits/text rendered.
- Merging wave64 emulation (#189) produced HUD/minimap and intro subtitles, but needed skips for
  `00167d58cc5a78d7` to avoid a device hang.
- A port on `Joaco9020/mangrove` plus `VICTORY_BVH_HOLD=1`,
  `VICTORY_SKIP_PIPELINES=00167d58cc5a78d7`, `VICTORY_POOL_NO_ASSERT=1` gave a visible G-buffer
  (prison yard, cars, characters) with 1-3.5 s frames, but lighting was dark/overexposed because
  ray-traced lighting does not run. This is the most promising branch for a second attempt.
- Cloned that branch read-only to `refs\mangrove2203` at
  `3bc3f5532394e12f282c1590af05cfd9a5eb61f9`; build script `build-mangrove.cmd`, config
  `victory-mangrove.toml`, warm script `warm-mangrove.cmd`, launcher `run-mangrove.cmd`.
  Built successfully: `build-mangrove.cmd` → exit 0,
  `target-mangrove\diag\victory.exe` 10,085,888 bytes, 2026-11-25 15:56:59.
  Warm-up finished (`logs\warm_mangrove.out`): 25,648 shaders translated, 58 refused;
  1,782 compute pipelines, 893 refused; 454 s; library 248,200 bytes;
  `victory-data\cache-mangrove` = 25,650 files / 1,288,633,886 bytes.
  Full trial `trial-mesh580.ps1 -Tag mangrove_world`: reached 10 experimental fallback
  pipelines at 19:15:26 UTC, but the same `amdxc64.dll+0xdaa6f9` fault hit 7 s later
  (title frame 4532, `fbk_stry000_FBK_STRY000_INTRO`); no screenshot was possible because
  the process was already gone when the old script waited 20 s before capturing.
  Before the fallback the intro ran at 22-26 fps with 11 draws/frame and no dropped draws.
  `logs\run_mangrove_world.err` has 2 unhandled-exception lines, 0 device-removed,
  0 `E_OUTOFMEMORY`, 1 host pipeline refused, 0 `title slot past`.
  DRED did not report a device removal (the fault is an access violation on the CPU thread).
  Disassembly of the return addresses places the crash in `victory_gpu::pipeline::library::pipeline`
  (the `load` closure that calls into the saved pipeline library), not in the wave64 DAG itself.
  The library file `cache-mangrove\pipelines\host-pipelines.bin` was rewritten at 19:15:23,
  3 s before the first fallback load; the branch documents that replacing a mapped library
  can leave zeroed pages and a driver fault. A/B launcher `run-mangrove-buf.cmd` adds
  `VICTORY_LIBRARY_MAP=0` (read the library through a buffer, no mapped pages); trial
  `mangrove_buf` started after fixing the script to capture 2 s after detection and to
  report real elapsed time. Stable `run.cmd` remains untouched.

`mangrove_buf` result (`logs\run_mangrove_buf.err`, crash
`temp\victory-20261125-162947-4384.crash.log`): reached the fallback at 19:29:43 UTC after
10m02s, got 9 indirect fallback draws, then hit the **same** `amdxc64.dll+0xdaa6f9` fault
4 s later. The backtrace is byte-for-byte the same library-load path
(`victory.exe+0x360102` → `+0x17b1af` → `+0x1765ac` → `+0x31ea75`), so mapping is not the
cause. No screenshot was saved: `capture-screen.ps1` refused the windowless process; the
trial script now uses `capture-screen-any.ps1` for the world capture. Next A/B:
`run-mangrove-lock.cmd` adds `VICTORY_LIBRARY_LOCK=1` to serialise the library calls.

External corroboration (Lichii issue #462, 2026-11-08): another project sees a hard
`0xc0000005` inside AMD's compiler on **RX 580 / Polaris with the same driver
`31.0.21925.1001`** while compiling heavy shaders; the fault offset is stable across runs
and builds, `DISABLE_OPTIMIZATION` and cache deletion do not help, and the maintainers treat
it as a separate line from the commit-limit failures that a larger page file fixed on RDNA2.
That matches our repeated, byte-identical `amdxc64.dll+0xdaa6f9` fault. It makes an AMD
driver bug the leading explanation, not a bug in the wave64 fallback or our cache logic.

`mangrove_lock` result (`logs\run_mangrove_lock.err`): 14 fallback pipelines, 9 indirect
draws, first capture at 19:41:35 UTC (`logs\mangrove_lock_world.png`,
`Americas | VIctory 0 fps 2274.5 ms/frame`) — the image is a dark letterboxed intro frame
with subtitles, not the 3D world. 20 s later the same library-load fault killed the process;
same backtrace, 2 exception lines, no device removal or OOM. Serialising the library calls
does not help either.

Last experimental step: added `VICTORY_NO_PIPELINE_LIBRARY=1` to
`refs\mangrove2203\crates\victory-gpu\src\pipeline\library.rs::library()` (experimental
branch only; default unchanged) so `LoadPipeline` is never called and PSOs are made directly.
Rebuild passed (`build-mangrove.cmd`, exit 0, 10,086,400 bytes); launcher
`run-mangrove-nolib.cmd`.

`mangrove_nolib` result (`logs\run_mangrove_nolib.err`): 6 fallback pipelines, 0 indirect
draws, and the **same** `amdxc64.dll+0xdaa6f9` fault after 9m17s. The only difference is the
victory frame: `+0x35c602` (direct `Create*PipelineState`) instead of `+0x360102`
(`LoadPipeline`). The captured `logs\mangrove_nolib_world.png` is a grey/UI frame, not the
3D world. No device removal or OOM; 15 mesh translations refused (cyclic wave64 DAG),
1 host pipeline refused.

## Final conclusion
All cache/library A/Bs produced the same byte-identical AMD driver fault:
`VICTORY_LIBRARY_MAP=0`, `VICTORY_LIBRARY_LOCK=1`, and even
`VICTORY_NO_PIPELINE_LIBRARY=1`. On this RX 580 2048SP with driver `31.0.21925.1001` the
driver cannot create/load these heavy PSOs at all; the failure is in the AMD compiler/driver,
not in the wave64 fallback, the pipeline library, or our descriptor code. The fallback is
therefore a **proof of concept only**: it can record compute/vertex mesh draws, but it does
not reach stable gameplay or a verified 3D world on this card. Do not port it into `src`.

Final check of the stable build (`final-stable-check.ps1`, 90 s): `run.cmd` stayed alive,
`Americas | VIctory 23 fps 43.5 ms/frame`, 0 unhandled exceptions, 0 panics, 0 device
removals, 0 `E_OUTOFMEMORY`, 257 non-fatal `host pipeline refused` warnings;
`logs\final_stable_check.png` captured. `C:\Games\VItoriaBr\src` and
`target\release\victory.exe` were never modified by the experiments. Keep `run.cmd` as the
launcher.

