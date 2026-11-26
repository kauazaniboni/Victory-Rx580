# Experimental compute and vertex mesh fallback

This experimental branch ports the fallback onto upstream `940d32f`.
It preserves upstream title-memory translation, root UAVs, view-address and
write-mark tables, the helper-lane fix and range analysis. It does not restore
the earlier descriptor or shadow-memory implementation.

With the `diagnostics` feature, `VICTORY_MESH_COMPUTE=1` selects a compute
stage that writes geometry records followed by a vertex stage that pulls
triangle corners. `VICTORY_MESH_WAVE64_DAG=1` enables the checked common
schedule for eligible wave64 mesh programs. Unsupported control flow is
refused; this is not general wave64 emulation.

Private geometry uses UAV `u0, space3` and vertex SRV `t254` in the root-view
space. These avoid upstream's memory space and `t255` stride table. Root
parameters are appended after upstream memory and write-mark bindings.

This GTX branch also uses a one-million-view heap in both the translator
and runtime. Upstream's two-million clamp exceeded this GPU's allocation
limit. The null UAV, null SRV and final clamped sentinel are reserved outside
title and dynamic slots. Large title heaps still use the upstream policy of
omitting slots that exceed available space; accessing those slots requires
descriptor remapping that this branch does not yet implement.

`VICTORY_MESH_INDIRECT=1` additionally accepts one uncounted, 12-byte mesh
dispatch argument. A GPU converter checks dimensions, scratch capacity and
the shared submission budget, then emits compute and vertex draw arguments.
Unsupported shapes produce no work. Direct scratch is bounded to 16 MiB;
indirect scratch is bounded to 4 MiB. Scratch and arguments are reused per
command list and retained until its reset. No title argument readback is
needed for execution.

Native compute first-lane elections use a group exchange only where analysis
proves full EXEC, complete 64-lane groups, uniform group control flow and
sufficient shared memory. Divergent loops do not gain group barriers.

The optional `VICTORY_SNAPSHOT_POOL_MIB` setting caps only idle snapshot
allocations. Missing or invalid settings keep the upstream 2 GiB limit;
explicit settings range from zero to 1024 MiB. Active snapshots remain intact.

Synthetic verification includes exact geometry records, pixel outputs,
texture loads and samples, both halves of wave64, repeated elections and
uniform loops, indirect command reuse, rejected dimensions and budget
exhaustion. Hardware and WARP checks must run serially with game trials.
Passing them does not establish correct world rendering or gameplay FPS.

Two ordinary shader tests still fail on this GTX, with the same outputs on
untouched upstream `940d32f`: `a_heap_buffer_read_by_element_takes_the_views_stride`
and `gathers_at_a_level_match_the_hardware`. Their assertions are retained.
The first reads incorrect structured-buffer elements; the second gathers
incorrect texels. The complete test suite is therefore not green, even when
the fallback-specific tests pass. These remain rendering investigations.

The old baseline is preserved locally on `codex/gtx-validated-baseline`;
that branch is not part of this publication. The old executable, game files,
saves and caches are preserved separately. Local
trial tooling records the executable hash and verifies firewall blocks and
memory guards before launching; there is no fixed gameplay timer.

## Build and focused checks

Use the pinned Rust toolchain and the Windows/MSVC requirements in the README.
Build the diagnostics profile to enable the experimental switches:

```powershell
cargo build --locked --profile diag -p victory --features diagnostics
cargo test --locked --profile diag -p victory-shader --lib uav_stage_tests -- --test-threads=1
cargo test --locked --profile diag -p victory-shader --lib compute_sample_tests -- --test-threads=1
cargo test --locked --profile diag -p victory-shader --lib view_image_tests -- --test-threads=1
```

The three filters above run the ordinary tests. Explicit GPU tests are ignored
by default; run these serially, with other games and GPU tests closed:

```powershell
cargo test --locked --profile diag -p victory-shader --lib compute_sample_tests -- --ignored --test-threads=1 --nocapture
cargo test --locked --profile diag -p victory-shader --lib view_image_tests -- --ignored --test-threads=1 --nocapture
```

Check the output for actual hardware and WARP execution: an unavailable
adapter can be skipped. A current `dxil.dll` is needed for validation, as
described in the README. No title files are needed by these synthetic tests.

For a separately guarded title trial, the experimental configuration is:

```powershell
$env:VICTORY_MESH_COMPUTE = '1'
$env:VICTORY_MESH_WAVE64_DAG = '1'
$env:VICTORY_MESH_INDIRECT = '1'
$env:VICTORY_SNAPSHOT_POOL_MIB = '128'
```

Use `target\diag\victory.exe` with the README's run arguments and your own
external data paths. These variables do not install firewall rules or process
memory limits. The local guarded trial runner is not included in this branch.

## Image metadata and validator fixes

The per-view metadata record now contains eight dwords. Buffer address words
are preserved; texture views also supply dimensions, levels and type. Placed
textures are inspected without eagerly allocating their host textures. The
translator permits the added fields only when it proves the bits being read
are represented. Other descriptor reads remain refused.

Shaders outside the pixel and compute stages declare the DXIL "UAVs at every
stage" flag when their module declares UAVs, including write marks. Heap-only
UAV access does not set that flag. Compute shaders using implicit derivatives
declare validator 1.8 and the matching PSV runtime-info version 3; other stages
and compute programs without derivatives retain the previous format.

## Recorded validation, 2026-11-25

The final local executable's SHA-256 is
`757ac37856f0d563c0c0c226f02f8fc3e24af887d20b47f5340d1a4a888b25f2`.
This identifies the tested binary; compiler inputs and paths can make another
build's hash differ. Binaries and title-derived data are not included here.

- The five new validator regression tests pass. Reverting the relevant fix
  makes the two bug-reproducing tests fail.
- The shader library suite: 274 passed, 2 known upstream failures, 11 ignored.
  The explicit compute-sampling GPU test separately passes on GTX 1070 Ti and
  WARP, with the expected sampled value. The image-view GPU test also passes
  on both adapters.
- Earlier fallback validation on the port: four explicit mesh tests, two
  election tests, four indirect tests on GTX and four on WARP passed. The
  image metadata update was followed by mesh and indirect regression checks.
- Clippy passes for the workspace/all-targets in default and all-features
  configurations with `-D warnings`. The upstream tree has unrelated rustfmt
  differences; no tree-wide formatting cleanup is included.
- Offline census: 14,240 vertex/pixel programs emitted and validated out of
  14,247; 2,621 compute programs emitted and validated out of 2,679; 8,694 mesh
  programs validated out of 8,765 emitted. The last validator fixes add 867
  validated vertex and 735 compute programs, with no other census outcomes
  changing. The preceding image metadata fix adds 35 pixel programs.

The census does not create title pipelines, exercise the GTX fallback for
every program, or prove rendering correctness. Remaining translation and
validation refusals include unsupported resource addresses, lane-mask/data
joins and mesh output-size limits. D3D12 debug-layer validation was unavailable
on the test PC.

The first title run of this exact binary initialized D3D12, then the external
system-commit guard stopped it after 14.8 seconds. No frames were captured and
no world-rendering or gameplay FPS result was established. That run cannot
establish whether the new shaders improve the visible scene.

Implementation, review and validation used Codex and Claude assistance.
