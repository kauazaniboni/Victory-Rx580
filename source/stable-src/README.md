# VItoriaBr

> 🇧🇷 **Português** | [🇺🇸 **English below**](#english)

Camada nativa de execução em Windows para **um título Xbox Series X|S**, rodando diretamente a partir dos arquivos que o próprio usuário possui.

O código x86-64 original do título executa inalterado dentro do nosso processo; o **VItoriaBr** fornece a plataforma Xbox/GDK por baixo e traduz os dados e shaders do console (D3D12.X / GFX10 → D3D12 / DXIL). Veja [docs/architecture.md](docs/architecture.md).

> **Base e licença:** Fork/continuação baseada no projeto open-source [Marvisak/VIctory](https://github.com/Marvisak/VIctory), com integração das melhorias GPL-3.0 de renderização do mundo 3D, iluminação, relay/VRAM e otimizador DXIL de [fxceless/victory-fork](https://github.com/fxceless/victory-fork), mantido sob a licença **GPL-3.0-or-later** ([LICENSE](LICENSE)). **Este repositório não contém nem distribui nenhum arquivo de jogo** — todos os caminhos de jogo, cache e dados devem ficar fora do repositório ([docs/asset-policy.md](docs/asset-policy.md)).

---

## Status e Progresso (2026-11-24)

**Ainda não jogável no mundo 3D.** O título inicia, roda os menus e vídeos a 30 fps cravados, salva perfil/configurações e carrega o mundo 3D, onde ainda ocorrem descartes de shaders não traduzidos e timeouts (TDR) em submissões pesadas de `DispatchMesh`.

### Testes reais em hardware (NVIDIA Blackwell)
Testado em: **Windows 11 Pro (build 26300)** · **Ryzen 7 5700X** · **40 GB RAM** · **NVIDIA RTX 5060 Ti 16 GB** (SM 6.6 + Mesh Shaders, driver `621.24`), sobre dump local Xbox Series `1.0.2.1`.

| Estágio | Estado |
|---------|--------|
| Boot, telas iniciais (acessibilidade, idioma PT-BR, configurações, legal) | ✅ Funciona — **30 fps** |
| Vídeo de introdução com áudio | ✅ Funciona (render limpo) |
| Tela inicial, seletor de conta, perfil e saves persistentes | ✅ Funciona |
| Tela de loading entrando no mundo (~50 s) | ✅ Funciona |
| Mundo 3D (cutscene de abertura e prólogo) | 🟡 **Renderizando ao vivo na RTX 5060 Ti (`~3 fps` no 1º run com `VICTORY_RELAY=2`, sem TDR)** — áreas na sombra ainda escuras (RT bounce light em progresso) |

### Cobertura de Shaders & PSOs medida na RTX 5060 Ti (`1.0.2.1`)
Medido com `victory translate --validate --pipelines` (`dxil.dll` Windows SDK `10.0.26100.0` + driver NVIDIA `621.24`):
- **Containers analisados:** `26.599` (`26.148` programas traduzidos para IR e 100% estruturados — `40,6 milhões` de instruções IR).

| Estágio | DXIL emitidos | Validados (`dxil.dll`) | PSOs aceitos (RTX 5060 Ti) | Taxa (PSOs / emitidos) |
|---------|---------------|------------------------|----------------------------|------------------------|
| **Vertex + Pixel** | `14.152 / 14.259` | `14.041` | **`14.152`** | **100,0%** |
| **Mesh** | `8.762 / 8.765` | `8.657` | **`8.762`** | **100,0%** |
| **Compute** | `2.715 / 2.734` | `2.715` | **`2.326`** | **85,7%** |
| **Total** | **`25.629 / 25.758` (99,5%)** | **`25.413`** | **`25.240`** | **98,5%** |

> Os únicos `389` PSOs recusados pela RTX 5060 Ti são compute shaders que exigem **wave64** (GPUs NVIDIA são wave32).

### Melhorias já aplicadas no VItoriaBr
- **Rastreamento completo de estado de `Shadow` buffers no `HostList`** (`crates/victory-gpu/src/{shadow,host,indirect,draw}.rs`):
  - Antes, `indirect::done()` deixava `title buffer` compartilhados presos em `D3D12_RESOURCE_STATE_UNORDERED_ACCESS`, causando erros de validação em `IASetVertexBuffers` e `id=527` no D3D12 Debug Layer.
  - Agora cada `Shadow` buffer rastreia seu estado via `prelude()` e transições intra-lista (`GENERIC_READ` para VB/IB/CBV/SRV/Indirect `args`+`counter`, `UNORDERED_ACCESS` para UAV/clamp), **zerando os erros de barreira no D3D12 Debug Layer** e estendendo significativamente a execução dentro do mundo 3D na RTX 5060 Ti.
- **Suporte a `MIMG` `a16` e operações LDS de byte/half** (`2471a62`, `crates/victory-shader/src/emit/lower.rs`):
  - `image_sample_b`, `_cl`/`_o`, `image_gather4_l` e leitura/escrita thread-safe de bytes/halves em LDS (`ds_read_{u8,i8,u16,i16}`, `ds_write_{b8,b16}{,_d16_hi}`).
- **Reconciliação de modos de interpolação DXIL por row da assinatura** (`5943771`, `crates/victory-shader/src/emit/shader.rs`):
  - Elementos que compartilham a mesma row na assinatura passam a emitir o mesmo modo de interpolação, **zerando ~48 erros do validador `dxil.dll`**.
- **Reciclagem imediata de slots dinâmicos do descriptor heap** (`a65d054`, `bd33acc`, `crates/victory-gpu/src/{descriptor/host,draw,list,queue}.rs`):
  - Os slots voltam ao heap assim que o `host.execute` síncrono termina, e o `MIN_DYNAMIC` sobe para **`2^18` (262.144)** em heaps limitados (NVIDIA, limite de 1.000.000 views), **zerando os 16.384+ draws/dispatches descartados** no mundo 3D.
- **Cubemap `image_sample_d_cl`** (`77b4b4c`, `crates/victory-shader/src/emit/lower.rs`):
  - `deriv_dwords` ajustado para `grads = 2` em cubemaps (`kind == 5`), destravando +27 shaders.
- **Estabilidade de Mesh Shaders, LDS e barreiras na NVIDIA Blackwell (wave32)** (`bd33acc`, `a13a719`, `1ece3a6`, `911267c`, `808a209`, `crates/victory-shader/src/emit/lower.rs`):
  - `mesh_prologue` limita `setMeshOutputCounts` ao número de threads para não deixar primitivas não-inicializadas; `emitIndices`, `storeVertexOutput` e `storePrimitiveOutput` validam limites contra `max_vertices`/`max_primitives`.
  - Todos os acessos a `LDS` (`ds_read`, `ds_write`, atomics) fazem bounds check fiel ao RDNA2 (OOB lê `0` e descarta escrita, evitando traps de SRAM na NVIDIA).
  - `s_barrier` em blocos que não pós-dominam a entrada, dentro de loops ou após `abort_block` emite apenas memory fence (`TGSM_FENCE`/`UAV_FENCE`), prevenindo deadlocks de `BAR.SYNC` em wave32.
- **Proteção anti-TDR em submissões e `ExecuteIndirect`** (`1ca533d`, `8a75883`, `6f99f75`, `crates/victory-gpu/src/{host,indirect}.rs`):
  - `host::execute()` submete 1 command list por vez com fence wait (evitando acumular >2 s de TDR); `indirect.rs` zera argumentos `DispatchMesh` corrompidos e limita dispatches válidos (`4096` grupos, `BUDGET = 2^22`).
  - `VICTORY_SHADER_LOOP_LIMIT` habilitado também em `release` (com padrão `4096`).

> **Aviso de GPU (TDR):** entrar no mundo 3D em uma GPU real ainda pode disparar reset do driver de vídeo (TDR). Para inspecionar sem risco de travar o driver, use o perfil `diag` com `VICTORY_D3D12_WARP=1` (adaptador em software).

---

## Como compilar e executar

**Requisitos:** Windows 10/11 x64, CPU com AVX2, GPU Direct3D 12 com Shader Model 6.6 + Mesh Shaders, Rust (`rust-toolchain.toml` — toolchain `x86_64-pc-windows-msvc`), VS 2022 MSVC e Windows SDK.

> O título exige vários GiB de memória commit livre no Windows. Se `crashes.log` reportar erro `1455`, feche outros programas ou aumente o arquivo de paginação.

```powershell
# 1. Configurar caminhos (sempre FORA do repositório)
copy victory.example.toml victory.toml
# Edite victory.toml com game_dir, cache_dir e data_dir

# 2. Validar configuração e testes do workspace (não precisa de dados do jogo)
cargo run -p victory -- config
cargo test --workspace

# 3. Compilar em release
cargo build --release -p victory
.\target\release\victory.exe run <pasta_do_jogo>\<executavel_do_titulo> --execute
```

### Build de diagnóstico (`diag`)
```powershell
cargo build --profile diag -p victory --features diagnostics
```
Principais variáveis de ambiente no `diag`:
- `VICTORY_D3D12_WARP=1` — força adaptador WARP em software (lento, não causa TDR)
- `VICTORY_D3D12_DRED=1` — grava breadcrumbs DRED em `crashes.log` se ocorrer `DEVICE_HUNG`
- `VICTORY_D3D12_DEBUG=1` / `VICTORY_D3D12_GBV=1` — D3D12 Debug Layer e GPU-Based Validation
- `VICTORY_CAPTURE=<pasta>` / `VICTORY_CAPTURE_FRAMES=<lista>` / `VICTORY_DUMP_TARGETS=1` — salva frames PNG e render targets
- `VICTORY_SHADER_LOOP_LIMIT=<n>` — limita iterações de loops em shaders traduzidos
- `VICTORY_SKIP_PIPELINES=<hash>,...` — pula draws/dispatches de hashes específicos

### Controles (teclado quando não há controle XInput conectado)
| Teclas | Controle |
|--------|----------|
| `W A S D` / `I J K L` | Analógico esquerdo / direito |
| Setas | D-pad |
| `Enter`, `Backspace`, `X`, `Y` | A, B, X, Y |
| `Q` / `E`, `Z` / `C` | LB/RB, LT/RT |
| `F` / `H` | L3 / R3 |
| `Esc`, `Tab` | Menu, View |

---

<a id="english"></a>

## English

### Overview

**VItoriaBr** runs **an Xbox Series X|S title** natively on Windows from files the user supplies.

The title's x86-64 code runs unchanged inside our process; **VItoriaBr** provides the underlying console platform and translates shaders/resources (D3D12.X / GFX10 → host D3D12 / DXIL). See [docs/architecture.md](docs/architecture.md).

> **Upstream & License:** Derived from [Marvisak/VIctory](https://github.com/Marvisak/VIctory) with GPL-3.0 world-rendering, lighting, relay/VRAM, and DXIL optimizer improvements integrated from [fxceless/victory-fork](https://github.com/fxceless/victory-fork), licensed under **GPL-3.0-or-later** ([LICENSE](LICENSE)). **No game files or derived assets are shipped or allowed in this repository** ([docs/asset-policy.md](docs/asset-policy.md)).

### Status & Progress (2026-11-24)

**Not playable in the 3D world yet.** Boot, menus, intro video with audio, profile/saves, and loading into the world work smoothly at 30 fps; inside the 3D world, untranslated shaders are still dropped and heavy `DispatchMesh` submissions can trigger a GPU TDR on real hardware.

#### Real Hardware Test Results (NVIDIA Blackwell)
Tested on: **Windows 11 Pro (build 26300)** · **Ryzen 7 5700X** · **40 GB RAM** · **NVIDIA RTX 5060 Ti 16 GB** (SM 6.6 + Mesh Shaders, driver `621.24`), Xbox Series build `1.0.2.1`.

| Stage | Status |
|-------|--------|
| Boot & first-boot screens (accessibility, language, settings, legal) | ✅ Works — **30 fps** |
| Intro video with audio | ✅ Works |
| Start screen, account selector, profile & persistent saves | ✅ Works |
| Loading into the world (~50 s) | ✅ Works |
| 3D world (opening cutscene & prologue) | 🟡 **Rendering live on RTX 5060 Ti (`~3 fps` on first run with `VICTORY_RELAY=2`, zero TDR)** — shadowed areas still dark (RT bounce light WIP) |

#### Measured Shader & PSO Coverage on RTX 5060 Ti (`1.0.2.1`)
Measured via `victory translate --validate --pipelines` (`dxil.dll` Windows SDK `10.0.26100.0` + NVIDIA driver `621.24`):
- **Containers scanned:** `26,599` (`26,148` programs translated & 100% structured — `40.6M` IR instructions).

| Stage | DXIL Emitted | Validated (`dxil.dll`) | Host PSOs (RTX 5060 Ti) | PSO Rate |
|-------|--------------|------------------------|-------------------------|----------|
| **Vertex + Pixel** | `14,152 / 14,259` | `14,041` | **`14,152`** | **100.0%** |
| **Mesh** | `8,762 / 8,765` | `8,657` | **`8,762`** | **100.0%** |
| **Compute** | `2,715 / 2,734` | `2,715` | **`2,326`** | **85.7%** |
| **Total** | **`25,629 / 25,758` (99.5%)** | **`25,413`** | **`25,240`** | **98.5%** |

> The `389` refused Compute PSOs are wave64-only shaders (NVIDIA GPUs run wave32).

#### VItoriaBr Improvements
- **Full `Shadow` buffer D3D12 resource state tracking in `HostList`** (`crates/victory-gpu/src/{shadow,host,indirect,draw}.rs`):
  - Previously, `indirect::done()` left shared title buffers stuck in `D3D12_RESOURCE_STATE_UNORDERED_ACCESS`, causing `IASetVertexBuffers` and barrier mismatch (`id=527`) errors in the D3D12 Debug Layer.
  - Each `Shadow` buffer now tracks its state across lists via `prelude()` and intra-list barriers (`GENERIC_READ` for VB/IB/CBV/SRV/indirect `args`+`counter`, `UNORDERED_ACCESS` for UAV/clamp), **eliminating all buffer state errors in the D3D12 Debug Layer** and significantly extending 3D world execution on the RTX 5060 Ti.
- **`MIMG` `a16` support and LDS byte/half operations** (`2471a62`, `crates/victory-shader/src/emit/lower.rs`):
  - `image_sample_b`, `_cl`/`_o`, `image_gather4_l`, plus thread-safe byte/half reads and writes in LDS (`ds_read_{u8,i8,u16,i16}`, `ds_write_{b8,b16}{,_d16_hi}`).
- **Per-row DXIL interpolation mode reconciliation on the signature** (`5943771`, `crates/victory-shader/src/emit/shader.rs`):
  - Elements sharing the same signature row now emit the same interpolation mode, **clearing ~48 `dxil.dll` validator errors**.
- **Immediate recycling of dynamic descriptor-heap slots** (`a65d054`, `bd33acc`, `crates/victory-gpu/src/{descriptor/host,draw,list,queue}.rs`):
  - Slots return to the heap as soon as the synchronous `host.execute` completes, and `MIN_DYNAMIC` rises to **`2^18` (262,144)** on capped heaps (NVIDIA, 1,000,000-view limit), **eliminating the 16,384+ dropped draws/dispatches** in the 3D world.
- **Cubemap `image_sample_d_cl`** (`77b4b4c`, `crates/victory-shader/src/emit/lower.rs`):
  - Uses `grads = 2` for `deriv_dwords` on cubemaps (`kind == 5`), unblocking +27 shaders.
- **Mesh Shader, LDS, and barrier safety on NVIDIA Blackwell (wave32)** (`bd33acc`, `a13a719`, `1ece3a6`, `911267c`, `808a209`, `crates/victory-shader/src/emit/lower.rs`):
  - `mesh_prologue` caps `setMeshOutputCounts` to thread count so no uninitialized primitives are left; `emitIndices`, `storeVertexOutput`, and `storePrimitiveOutput` enforce `max_vertices`/`max_primitives` bounds.
  - All `LDS` reads, writes, and atomics enforce RDNA2-accurate OOB semantics (read `0`, drop writes) to prevent NVIDIA shared-memory traps.
  - `s_barrier` in non-post-dominating, loop, or post-`abort_block` blocks emits a memory-only fence (`TGSM_FENCE`/`UAV_FENCE`), preventing `BAR.SYNC` deadlocks on wave32.
- **Anti-TDR submission chunking & indirect bounds** (`1ca533d`, `8a75883`, `6f99f75`, `crates/victory-gpu/src/{host,indirect}.rs`):
  - `host::execute()` submits one command list per fence wait; `indirect.rs` zeroes out-of-range `DispatchMesh` arguments and caps valid dispatches (`4096` groups, `BUDGET = 2^22`).
  - `VICTORY_SHADER_LOOP_LIMIT` enabled in `release` builds as well (default `4096`).

> **Warning:** Entering the 3D world on a real GPU can still trigger a display driver reset (TDR). Use a `diag` build with `VICTORY_D3D12_WARP=1` to inspect safely on the software adapter.

### Quick Start

```powershell
copy victory.example.toml victory.toml   # set paths outside the repository
cargo run -p victory -- config
cargo test --workspace
cargo build --release -p victory
.\target\release\victory.exe run <game_dir>\<title_exe> --execute
```

Diagnostics build:
```powershell
cargo build --profile diag -p victory --features diagnostics
```

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
