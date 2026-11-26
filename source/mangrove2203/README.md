# mangrove

**mangrove** runs the Xbox Series X|S (GDK) build of GTA VI natively on Windows PCs with
**8 GB NVIDIA graphics cards**, from game files you supply yourself.

mangrove continues the work of [Marvisak's VIctory](https://github.com/Marvisak/VIctory)
and [hyphennn's victory-fork](https://github.com/fxceless/victory-fork) with its
own updates, aimed at getting further on 8 GB NVIDIA cards:

- **Plays past the 8-minute stop** in the opening cinematic (VIctory #698) and
  reaches the prologue's gameplay: its own loader code disarms the title's
  pool-empty stops (`pool_asserts.rs`).
- **Light in the shade**: an ambient pass stands in for the ray-traced
  indirect light, so shadowed characters are not black.
- **Faster**: measured settings take the prologue from 1.6 to 2.6 fps.
- **Pre-warm** of every shader before playing, **keyboard and mouse** play with
  mouse-look, and an **easy-to-read status window**.

Upstream updates are merged as they land; mangrove's own changes are listed with
commit ids in [CHANGELOG.md](CHANGELOG.md).

> [!IMPORTANT]
> **This repository contains no game files, shaders, dumps or assets, and none
> will ever be provided — not here, not in issues, not by request.** You need
> your own copy of the game. Not affiliated with or endorsed by its developer
> or publisher.

## How mangrove differs from victory-fork

| On an RTX 3070 8 GB | victory-fork alone | mangrove |
|---|---|---|
| Opening cinematic | fork-based builds without the hook stopped about 6-8 minutes in, in every long run on this PC (3 of 3; `0x80000003`, pool full, the stop VIctory #698 reports on AMD, NVIDIA and Linux) | plays past it: `pool_asserts.rs` (mangrove's own code) disarms the 2,622 pool-empty stops it finds in the mapped title |
| Gameplay | not reached on this PC | reached: the prologue's HUD help prompt, pad input moves Lucia |
| `bcryptprimitives.dll!ProcessPrng` | ends the run when called | allowed (host policy) |
| Frame rate in the prologue | 1.6 fps (launcher settings of the 8 GB tips) | 2.5 fps (`pc\settings.bat`, measured over five overnight runs) |
| Before the first run | shaders translate while playing (black first run) | `victory warm` / `pc\warm.bat` pre-translates every shader and pre-builds compute pipelines |
| Keyboard and mouse | keyboard as a pad | plus mouse-look, mouse-button triggers, Tab/Space/Shift mappings |

mangrove's own changes are listed with commit ids in [CHANGELOG.md](CHANGELOG.md);
everything else is victory-fork, merged as it updates.

## Status (2026-11-25)

**GTA VI gameplay on an RTX 3070 8 GB:** after the opening cinematic the prologue hands control to the player: the HUD help prompt ("Approach a cellmate to interact with them") shows and keyboard pad input moves Lucia and the camera, about 45 minutes in, at 1-2 fps.

**GTA VI on an RTX 3070 8 GB:** boots through the intro and title screen at 30 fps, then the prologue's opening cinematic renders in 3D in the world, and keeps playing past the 8-minute stop (three overnight runs of 60-90 minutes, up to frame 11,000+, 2.3 fps with `pcsettings.bat`).

Tested on Windows 11 with an **NVIDIA GeForce RTX 3070 (8 GB)**, Ryzen 5 2600, 32 GB RAM:

| | |
|---|---|
| Boot, first-boot setup (subtitles, language), menus | ✅ works, 30 fps |
| Opening credits | ✅ render in colour |
| Gameplay (prologue, prison yard) | ✅ the player has control: HUD help prompt, keyboard pad input moves Lucia (1-2 fps) |
| The 8-minute stop (pool full, VIctory #698) | ✅ **passed with `VICTORY_POOL_NO_ASSERT=1`**: three runs of 60-90 min, up to title frame 11,298 (reports stop at 920-1,480), no crash (RTX 3070 8 GB, 2026-11-25) |
| Opening cinematic | 🟡 **renders in 3D** once the shader cache is warm; flickers in places |
| Frame rate | 🟡 30 fps in menus; 2.5 fps in the prologue (RTX 3070, Ryzen 5 2600) |
| Known problems | ❌ the game's ray-traced lighting does not run yet: with `VICTORY_AMBIENT` (on by default) shadowed areas get a constant light instead of black; subtitles flicker; the first run is mostly black while shaders and pipelines are built (they are cached after that, so each run shows more) |

Screenshots and video stay out of this repository (no game content, per
[docs/asset-policy.md](docs/asset-policy.md)); they are posted separately.

## What mangrove adds

| | |
|---|---|
| `pc\play.bat` | one-click launcher with the 8 GB NVIDIA settings below |
| `pc\easy-view.ps1` | replaces the raw log with a plain-English window: speed, objects drawn, objects skipped (not ready yet), shaders ready, files loaded, and red **PROBLEM** lines for GPU resets or out-of-memory |
| `victory warm` | pre-translates every shader and pre-builds the compute pipelines before you play (ported from leonida-victory), so the first run is not mostly black |
| `VICTORY_AMBIENT=<level>` | a constant indirect light in place of the title's ray-traced probes (which do not run), so what the sun does not reach is not pitch black; 0.6 by default, `off` for the title's own output (from Belobug/VIctory-Belobug-AMD) |
| `VICTORY_POOL_NO_ASSERT=1` | gets past the crash about 6-8 minutes into the opening cinematic (below); on in `play.bat` |
| `pc\second-monitor.ps1` | puts the game window on your second monitor at 1280×720 without stealing focus |

Settings used by `play.bat`:

| Switch | Why |
|---|---|
| `VICTORY_TEXTURE_GUARD=0` | stops repeated/flickering frames on NVIDIA (fork issue #269) |
| `VICTORY_SHADER_OPT=0` | removes black mosaic blocks (#310) |
| `VICTORY_TEXTURE_ROOM_PERCENT=94` | more of an 8 GB card for textures (textures waiting for room: 25,846 -> 16,357) |
| `VICTORY_LATE_WAIT=2` | settles GPU copy-backs later: 2.3 fps instead of 1.5, copy-back 218 ms instead of 521 per frame, nothing visibly missing |
| `VICTORY_LOG=info,…descriptor::host=error` | mutes one warning NVIDIA repeats ~300,000 times |

## Getting past the 8-minute crash

About 6-8 minutes into the opening cinematic the game stops itself (`0x80000003`,
exit code -2147483645) on every worker thread at once. The title's own crash
report shows its `INSTANCE_LIST` pool full (45000/45000); the stop is an `int3`
in the game's lock-free pool allocator when the free list is empty (the
free-list head reads the end marker `0x1fffe`). It happens at a fixed point in
the cinematic, not after a time-out, and more RAM or a bigger paging file does
not help: the pool size is fixed in the game. Tracked in
[VIctory #698](https://github.com/Marvisak/VIctory/issues/698).

`VICTORY_POOL_NO_ASSERT=1` (in `crates/victory-loader/src/pool_asserts.rs`)
writes a NOP over each pool-empty `int3` right after the game is mapped, so the
allocation returns null instead, as the allocator already does on its own
early-out. The approach is WalliceCariasPerussio's (17+ minutes in the world
on an RTX 5060). mangrove's version finds the sites itself in the mapped code
(every allocator copy pops its free list with a 17-bit `and r32, 0x1ffff` next
to a `lock cmpxchg`; each nearby jump landing on `int3` + `xor r, r` is a site),
so **no list derived from the game is shipped**. It runs only on the build it
was checked on, and it is a workaround: a caller that does not expect null
could still fault later.

### How the stop was found

- Every worker thread stopped at once on `0x80000003` at the same address, with
  `rax = 0x17690001fffe`. Disassembling the title around that address shows a
  lock-free free-list pop: the head's low dword is masked with `0x1ffff`, swapped
  in with `lock cmpxchg`, and compared against `0x1fffe` first. `0x1fffe` is
  the list's end marker, so the pool was empty and the allocator fell into its
  `int3`.
- The instruction after the `int3` is `xor reg, reg` on the way to the
  function's own null return, so a NOP there turns the stop into a failed
  allocation.
- VIctory #698 tied the same address to the title's own crash report
  (`INSTANCE_LIST: 45000/45000`, SRL `Prestream`) and found the stop at a fixed
  point in the cinematic on AMD, NVIDIA, Windows and Linux alike.
- The title has 5,776 inlined copies of the allocator; scanning for the 17-bit
  pop next to `lock cmpxchg` and following nearby jumps onto `int3` + `xor r, r`
  finds 2,622 pool-empty sites (a superset of the 2,205 a full disassembly
  reports).

## Known limits

- **About 340 draws per frame are dropped** in the prologue. They are not
  translation failures (only 2 compute shaders fail to translate). The title
  creates those pipelines through two D3D12.X device methods (vtable slots
  `0x1c8` and `0x218`) that no PC layer implements yet. Logging what their
  descriptions point at (946014c) shows a root signature, a 0x180-0x340-byte
  block in GPU memory (the console's precompiled native shader code), a
  0x118-0x1d8-byte state block and two further title structures, and no DXBC
  containers: writing them means translating native GFX10 programs and
  decoding that state block.
- **Shadows and indoor light are black**: the title's ray-traced lighting is
  not emulated.
- **An 8 GB card runs out of texture room**: about 16,000 textures wait for
  room at any time in the prologue (`VICTORY_TEXTURE_ROOM_PERCENT` trades
  buffer room for texture room).
- **2-2.5 fps** in the prologue on an RTX 3070 / Ryzen 5 2600.

## Troubleshooting

| You see | What it is | What to do |
|---|---|---|
| The game closes about 6-8 minutes into the opening cinematic, exit code `-2147483645` (`0x80000003`) | the title's pool allocator stops on an empty `INSTANCE_LIST` pool (VIctory #698) | keep `VICTORY_POOL_NO_ASSERT=1` (on in `play.bat`) |
| The first run is mostly black, objects pop in | shaders and pipelines are still being built | run `victory warm` once, then let the first run play; every run after starts warmer |
| "unimplemented import `bcryptprimitives.dll!ProcessPrng`" right after start | a Windows random-number function the title calls | fixed in mangrove: the host's `bcryptprimitives.dll` is allowed |
| Windows error 1455 in `crashes.log` | Windows ran out of commit memory | raise the paging file (the title commits about 20 GB) |
| The easy view says "card has no room", textures stay blurry or blank | an 8 GB card is full; textures wait their turn | close other GPU programs; `VICTORY_TEXTURE_ROOM_PERCENT` (50-97, default 90) trades buffer room for texture room |
| A yellow "driver refused one effect program" note | a few effect pipelines the NVIDIA driver rejects | harmless: the game skips that effect |
| Shadows and indoor areas are black | the title's ray-traced lighting is not emulated yet | nothing yet; no PC build runs it |

## Requirements

- Windows 10/11 x64, a CPU with AVX2
- An NVIDIA RTX card (20 series or newer: shader model 6.6 and mesh shaders), 8 GB or more
- 32 GB RAM recommended; a large Windows paging file (error 1455 means it is too small)
- Rust (see `rust-toolchain.toml`)
- Your own copy of the game

## Build

```powershell
cargo build --release -p victory
```

## Play

1. Open `pc\play.bat` in a text editor and set `GAME=` to your own game executable.
2. Once (and after each update): set `GAME=` in `pc\warm.bat` too and double-click it. It pre-translates every shader and pre-builds the compute pipelines with `play.bat`'s settings (`pc\settings.bat`), so the first run is not mostly black.
eleaseictory.exe warm <game folder>\shaders.rpf <game folder>\game_scarlett_master.exe` (with the same `VICTORY_*` settings and cache folder as `play.bat`) to pre-build shaders.
3. Double-click `pc\play.bat`.
4. Leave the opening cinematic running for a while on the first run: the 3D
   fills in as pipelines are built and saved. The **Skipped** number in the easy
   view drops run after run.

Controller: XInput pads (a DualShock/DualSense through DS4Windows works). With
no pad, the keyboard and mouse act as one while the game window has focus:

| Pad | Keyboard / mouse |
|---|---|
| Left stick | W A S D |
| Right stick (camera) | mouse (the cursor stays in the window; Alt+Tab frees it; `VICTORY_MOUSE_LOOK=0` turns it off), or I J K L |
| A / B / X / Y | Enter or Left Shift / Backspace / X or Space / Y |
| LT / RT | right mouse button, Tab or Z / left mouse button or C |
| LB / RB | Q / E |
| L3 / R3 | F / H |
| D-pad | arrow keys |
| Menu / View | Esc / V |

## Credits

mangrove only adds a launcher, the easy view and 8 GB NVIDIA settings. **All of
the emulation work belongs to these people:**

| Who | What |
|---|---|
| **[Marvisak](https://github.com/Marvisak)** and the [VIctory contributors](https://github.com/Marvisak/VIctory/graphs/contributors) | [VIctory](https://github.com/Marvisak/VIctory): the compatibility layer itself (PE loader, kernelx / xgameruntime stand-ins, RDNA2 shader translation to DXIL, D3D12.X graphics layer) |
| **hyphennn** ([fxceless](https://github.com/fxceless)) and the [victory-fork contributors](https://github.com/fxceless/victory-fork/graphs/contributors) | [victory-fork](https://github.com/fxceless/victory-fork): the rendering and speed fixes mangrove is built on |
| **[Zettrox](https://github.com/Zettrox)** | [ztrx](https://github.com/Zettrox/ztrx): showed this code reaching the world on an 8 GB NVIDIA card (RTX 3050) |
| **[mauri9911](https://github.com/mauri9911)** and myokex | [leonida-victory](https://github.com/mauri9911/leonida-victory): testing help, NVIDIA shortcuts and the `victory warm` command ported here |
| **[Belobug](https://github.com/Belobug)** | [VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD): the ambient-light pass (`ambient.rs`) ported here |
| **[WalliceCariasPerussio](https://github.com/WalliceCariasPerussio)** | the pool-assert workaround (VIctory #698) that `VICTORY_POOL_NO_ASSERT` re-implements |
| The VIctory #698 reporters (Sil3ntGam3r, LAKSHYA-cmd-tech, likecrank-dotcom, VibeClauding, VoidDeficit, Sunkiller-qc and others) | pinned the 8-minute stop to the `INSTANCE_LIST` pool |
| The people who reported victory-fork issues #269 and #310 | found the texture-guard and shader-optimiser switches used in `play.bat` |
| **[4lwayz](https://github.com/4lwayz)** | [PortWatch VI](https://portwatch-vi.xyz), which tracks all of these efforts |

What mangrove changed, with commit ids: [CHANGELOG.md](CHANGELOG.md). The fork's own README is kept in
[`docs/victory-fork-README.md`](docs/victory-fork-README.md).

Made with AI assistance ([Claude Code](https://claude.com/claude-code)).

## License

GPL-3.0, as upstream. See [LICENSE](LICENSE).
