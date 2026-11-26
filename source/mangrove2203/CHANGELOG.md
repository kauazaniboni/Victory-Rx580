# Changelog

What mangrove adds on top of [hyphennn's victory-fork](https://github.com/fxceless/victory-fork)
(merged as it updates). Tested on an RTX 3070 8 GB, Ryzen 5 2600, 32 GB RAM, Windows 11.

## 2026-11-25

### Gets further
- `VICTORY_POOL_NO_ASSERT=1`: gets past the stop about 6-8 minutes into the
  opening cinematic (`0x80000003`, `INSTANCE_LIST` 45000/45000, VIctory #698):
  finds the title's pool-empty `int3` sites in the mapped code (2,622) and
  writes NOPs so the allocation returns null. No game-derived list is shipped.
  Approach by WalliceCariasPerussio. (2995c9d)
- Host policy allows `bcryptprimitives.dll` (`ProcessPrng`); an unbound call
  ended a run. (0dd50f9)
- Result: the prologue reaches gameplay (HUD help prompt, pad input moves
  Lucia); three overnight runs of 60-90 minutes, up to frame 11,298, no crash.

### Picture
- Ambient light (`VICTORY_AMBIENT`, 0.6 by default): a constant stands in for the
  title's ray-traced indirect light right after its probe resolve, so shadowed
  areas are not pitch black. Ported from Belobug/VIctory-Belobug-AMD.

### Faster, more on screen
- `pc\settings.bat`: `VICTORY_LATE_WAIT=2`, `VICTORY_TEXTURE_ROOM_PERCENT=94`,
  fork-default card buffers. Measured: 1.6 -> 2.3 fps, copy-back 521 -> 218 ms
  per frame, textures waiting for card room 25,846 -> 16,357. (df4de41)
- `victory warm` and `pc\warm.bat`: pre-translate every shader and pre-build the
  compute pipelines before playing (ported from leonida-victory 4ec99d0).
  (5959db7, 4c428d6)

### Play
- `pc\play.bat`: one-click launcher with the 8 GB NVIDIA settings. (5101fce)
- Keyboard and mouse as a pad: mouse-look on the right stick, mouse buttons as
  triggers, Tab = LT, Space = X, Left Shift = A, View on V. (f4274bc, tests 29f0f2b)
- `VICTORY_KEYS` hold time (`<seconds>:<key>:<hold seconds>`) for scripted
  runs at 1-2 fps. (d46519c)
- The game window goes to the second monitor (`pc\second-monitor.ps1`).

### Easy view
- `pc\easy-view.ps1` replaces the raw log with a plain-English status line
  (frame, fps, objects drawn and skipped, textures waiting for card room,
  shaders, pipelines, files) and red alerts for real problems only.
  (5101fce, 4d7dbbf, 4ca4cdc, e52e4e0)

### Repository
- CI: clippy fixes on fork code (afccb65); the depth-bounds GPU test skips on
  software adapters, which CI runners have (fb04e71); the asset policy allows
  `.bat` and keeps screenshots out (6c5efb0).
- README: status, troubleshooting, how the 8-minute stop was found, credits.
