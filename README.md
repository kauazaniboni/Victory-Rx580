# VIctory RX 580 — progresso completo

Repo autocontido no estilo dos forks enviados: **código-fonte + histórico + scripts + config + evidências**, pronto para o GitHub. Snapshot atualizado em 26/11/2026.

> Nada aqui altera `C:\Games\VItoriaBr\run.cmd`, `target\release\victory.exe` nem o dump do jogo (208 GB). O build estável e o jogo aberto foram preservados.

## O problema

GTA 6 roda no SSD em `C:\Games\VItoriaBr`; áudio, input e lógica funcionam, mas a imagem fica preta. Log: `~2 fps`, `~850 draws/frame`, `~1579` descartados sem host pipeline, `~5694` draws indiretos de mesh e `0` dispatches executados.

Causa: **RX 580 2048SP é `FL12_0 / SM6.6 / MeshShaderTier=0`**. O runtime estável recusa os ~8762 shaders de mesh (`pipeline/host.rs:359`). A CPU manda; a GPU não desenha a geometria. O artefato antigo `A→B→A` é pendente e separado.

## O que já foi descartado

- `VICTORY_CARD_BUFFERS_MIN_MB=1` e `VICTORY_RELAY=2`: não corrigem o `A→B→A`.
- Heap-sync do commit `86a5ed6`: reproduziu o rollback no frame `1491`.
- Fallback mesh→compute puro (PR #2203 / mangrove): morre no compilador AMD `amdxc64.dll+0xdaa6f9`, no lote com `compute 052a2316a03b9ba1` + seis pares vertex/pixel.
- Captura GDI/PrintWindow não valida flip model; use captura interna com build `diagnostics`.
- `ruff-lab` / `klbf-lab` jogam de verdade, mas exigem mesh shader (RX 6000+); não resolvem RX 580 diretamente.
- `VICTORY_MESH_TEST=skip|scissor` só remove/clipa draws: não é correção de imagem.

## Caminho em teste

Fallback mesh→compute **com pré-compilação offline**:

```cmd
victory translate --cache --pipelines --jobs 6 <shaders.rpf> <exe> <dlls>
```

`source/gtxfallback` é o único tree local com `--cache`. Ele salva a biblioteca a cada 20 s e retoma após crash. `VICTORY_SKIP_PIPELINES` atua no jogo, não no prebuild; para pular um pipeline que trava o compilador, adiciona-se a chave dele ao `prebuilt-pipelines.txt` do cache. Procedimento em `RUNBOOK-mesh580.md`.

Mapa de chaves gerado com `list-mesh580-keys.cmd`: 28.164 pipelines (25.471 graphics, 2.693 compute). Helpers: `scripts\find-mesh580-key.ps1 -Hash <hash>` e `scripts\skip-mesh580-key.ps1 -Key <chave> -Hashes <hashes>`.

## Estrutura

| Caminho | Conteúdo |
|---|---|
| `README.md` | Este resumo |
| `HANDOFF.md` | Histórico integral das conclusões e A/Bs anteriores |
| `RUNBOOK-mesh580.md` | Prebuild → skip → verificação visual |
| `ATTRIBUTION.md` | Créditos dos forks e da técnica de prebuild |
| `source/gtxfallback/` | Fork com mesh→compute + `translate --cache` (HEAD em `evidence/source-heads.txt`) |
| `source/mangrove2203/` | Variante mangrove do fallback #2203 |
| `source/stable-src/` | Árvore estável usada como baseline |
| `scripts/` | Launchers, prebuild, build, captura, análise e helpers de chave |
| `config/` | `victory.toml`, `victory-mesh580.toml`, `victory-mangrove.toml` |
| `evidence/` | Recortes de log, crash AMD, hashes SHA-256, inventário de cache, HEADs, `black_check.png` |
| `LICENSE` | GPL-3.0 (de `gtxfallback`) |

**Não incluído:** dump do jogo, binários, caches inteiros, `.git` aninhados nem o mapa de chaves de 91 MB — só recortes e hashes.

## Build

```powershell
call C:\Games\VItoriaBr\toolchain\BuildTools\VC\Auxiliary\Build\vcvars64.bat
set RUSTUP_HOME=C:\Games\VItoriaBr\toolchain\rustup
set CARGO_HOME=C:\Games\VItoriaBr\toolchain\cargo
set PATH=C:\Games\VItoriaBr\toolchain\cargo\bin;%PATH%
set CARGO_TARGET_DIR=<seu target>
cd source\gtxfallback
cargo build --locked --profile diag -p victory --features diagnostics
```

O fallback exige `--features diagnostics` e `VICTORY_MESH_COMPUTE=1`, `VICTORY_MESH_WAVE64_DAG=1`, `VICTORY_MESH_INDIRECT=1`. O mesmo conjunto precisa ser usado no prebuild e no run, senão a chave do cache muda.

Nenhum push foi feito; os commits deste snapshot são locais.
