# RX 580 mesh→compute fallback + offline pipeline pre-build

Tudo aqui é separado do padrão: `run.cmd`, `target\release\victory.exe` e o jogo aberto não são alterados.

## Por que

A RX 580 reporta `MeshShaderTier=0`, então o runtime estável descarta os draws de mesh (tela preta com áudio/controle normais). O build `target-mesh580\diag\victory.exe` (PR #2203) converte mesh→compute+vertex, mas o compilador AMD (`amdxc64.dll+0xdaa6f9`) travava ao criar certos pipelines. O pré-build offline `translate --cache --pipelines` evita compilar durante o jogo, salva a biblioteca a cada 20 s e retoma após um crash.

## Ordem (com o jogo fechado)

1. `list-mesh580-keys.cmd` — só lista chaves/hashes; não cria pipelines. Saída: `logs\mesh580_prebuild_keys.txt`.
2. `prebuild-mesh580-run.cmd` — wrapper desanexado que chama `prebuild-mesh580.cmd` e redireciona stdout/stderr para `logs\mesh580_prebuild.out/.err`. Use-o para o comando não ser morto por timeout; o cache parcial é reaproveitado se interrompido.
3. Se travar no `amdxc64.dll`, pegue a última linha `built` de `logs\mesh580_prebuild_built.log` (ou o lote `slow host submission` no `.err`). Com um hash de 16 dígitos, rode `find-mesh580-key.ps1 -Hash <hash>` para achar a chave no mapa do passo 1 (o arquivo tem 91 MB e fica fora do repo). Lote conhecido do crash anterior: `compute 052a2316a03b9ba1` + seis pares vertex/pixel.
4. Rode `skip-mesh580-key.ps1 -Key <chave> -Hashes <hash1,hash2,...>`. Ele adiciona a chave como linha extra em `victory-data\cache-mesh580-pre\pipelines\prebuilt-pipelines.txt`, sem tocar na primeira linha (o tag do blob), e salva os hashes em `logs\mesh580_skip_hashes.txt`. Isso faz o pré-build pular aquele pipeline na próxima rodada.
5. Rode `prebuild-mesh580.cmd` de novo; ele retoma os já salvos e continua depois do pipeline pulado. Repita 3–5 se aparecer outro travamento.
6. Rode `run-mesh580-safe.cmd "<game dir>\game_scarlett_master.exe" "<hash1,hash2,...>"` com os hashes do passo 3 (o `VICTORY_SKIP_PIPELINES` do jogo omite os draws daquele pipeline). Para A/B visual, use `run-mesh580-capture.cmd` no lugar dele: captura internal frames em `logs\capture-mesh580` (potências de dois + offsets 0,1,2,4,8,16,32,64,128 do primeiro frame com 500 draws).

## Verificação

Não use captura GDI/PrintWindow (não reproduz o flip model). Use `run-mesh580-capture.cmd`: ele ativa `VICTORY_CAPTURE` e `VICTORY_CAPTURE_WORLD` no build `diagnostics`, gravando PNGs em `logs\capture-mesh580` no mesmo ponto da cena. Compare com `evidence\black_check.png` e com os frames do baseline estável. Espere ver fallback mesh→compute nos logs, mais draws/frame e nenhum `amdxc64.dll`, `device removed`, `panic` ou `E_OUTOFMEMORY`. Só promover para `src` depois dessa verificação.

## Notas

- `VICTORY_CACHE_DIR` no config/aplicativo aponta para `cache-mesh580-pre`; prebuild e run **precisam** dos mesmos `VICTORY_MESH_COMPUTE=1`, `VICTORY_MESH_WAVE64_DAG=1`, `VICTORY_MESH_INDIRECT=1`, senão a chave do cache muda (`-wave64-dag-` entra no hash).
- `VICTORY_SKIP_PIPELINES` é lido pelo jogo, não pelo prebuild; por isso o passo 4 usa a lista `prebuilt-pipelines.txt`.
- `VICTORY_MESH_TEST=skip|scissor` e `VICTORY_SKIP_PIPELINES` sozinhos apenas removem/clipam draws: não são correção de imagem.
