# Experimental compute and vertex mesh fallback

This local branch ports the validated fallback onto upstream `940d32f`.
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

This branch keeps the old baseline on `codex/gtx-validated-baseline`. The
old executable, game files, saves and caches are preserved separately. Local
trial tooling records the executable hash and verifies firewall blocks and
memory guards before launching; there is no fixed gameplay timer.
