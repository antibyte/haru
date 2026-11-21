# M0 feasibility: Haru-4B Q6_K per-frame compute

Date: 2026-11-20. Gate: p50 per-frame compute < 133 ms is required (real time), < 100 ms is the target.

## Machine

- Windows 11 Pro, 63.6 GB RAM, CPU 11th Gen Intel Core i7-11700B
- GPU Vulkan0: Intel Arc B580, driver 32.0.101.9150 (12.8 GB)
- GPU Vulkan1: Intel UHD Graphics, driver 32.0.101.7092 (iGPU, shared memory)
- Vulkan SDK 1.4.350.0; speech.cpp v0.11.1 Vulkan build `build\speech-vulkan\Release\speech.exe`
- Model: `Haru-4B-Q6_K.gguf` (5.6 GB)

`speech devices --json`:

```json
{"devices":[{"name":"Vulkan0","description":"Intel(R) Arc(TM) B580 Graphics","kind":"gpu","memory_total":12782141440,"memory_free":11976765440},{"name":"Vulkan1","description":"Intel(R) UHD Graphics","kind":"igpu","memory_total":34133055488,"memory_free":34133055488},{"name":"CPU","description":"11th Gen Intel(R) Core(TM) i7-11700B @ 3.20GHz","kind":"cpu","memory_total":68266110976,"memory_free":30583312384}]}
```

On this machine the B580 is `Vulkan0` (the plan examples say `Vulkan1`; device indices differ per machine, so the app must select by `speech devices`, not by a fixed name).

## Commands

```powershell
$SPEECH = "build\speech-vulkan\Release\speech.exe"
$MODEL  = "$env:LOCALAPPDATA\speech.cpp\models\sakasegawa--Haru-4B-GGUF\8db05e86acca046bc183d4e30fbda22ac775ba4767ae5a10bcd0d48f8d7db793\Haru-4B-Q6_K.gguf"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts\make-test-wav.ps1
$RUN = "cargo run --release --manifest-path app/src-tauri/Cargo.toml --example dialogue_smoke --"
# 1 B580 fast, 2 B580 realtime, 3 UHD fast, 4 CPU fast
& $RUN --speech $SPEECH --model $MODEL --wav build\test\question-de.wav --device Vulkan0 --mode fast     --out build\test\smoke-reply.wav
& $RUN --speech $SPEECH --model $MODEL --wav build\test\question-de.wav --device Vulkan0 --mode realtime --out build\test\smoke-reply-rt.wav
& $RUN --speech $SPEECH --model $MODEL --wav build\test\question-de.wav --device Vulkan1 --mode fast     --out build\test\smoke-reply-uhd.wav
& $RUN --speech $SPEECH --model $MODEL --wav build\test\question-de.wav --device cpu --tail-secs 2 --mode fast --out build\test\smoke-reply-cpu.wav
```

Input: German SAPI question (Microsoft Hedda/Katja/Stefan de-DE installed), 24 kHz mono 16-bit, plus a 6 s silent tail (CPU: 2 s), padded to whole 3200-sample frames (94 chunks = 12.53 s; CPU 64 chunks).

## Results

Compute = time from sending chunk k to receiving frame k+1 (frame 0 excluded). Frame budget is 133.3 ms.

| Device | Mode | Frames | Load s | First frame s | p50 ms | p90 ms | p99 ms | max ms | Over 133 ms | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|
| Vulkan0 (Arc B580) | fast | 95 | 8.69 | 1.00 | 34.8 | 68.2 | 166.5 | 166.5 | 1 | PASS |
| Vulkan0 (Arc B580) | realtime | 95 | 4.25 | 0.69 | 35.4 | 56.0 | 69.3 | 69.3 | 0 | PASS |
| Vulkan1 (UHD) | fast | 95 | 26.86 | 6.30 | 370.6 | 440.3 | 492.3 | 492.3 | 94 | FAIL |
| CPU (i7-11700B) | fast | 65 | 6.79 | 2.08 | 419.8 | 656.9 | 1371.8 | 1371.8 | 64 | FAIL |

- Realtime on the B580: playback never fell behind (max lag 0.00 s, i.e. each frame arrived before the next chunk was due). Load time 8.69 s on the first run includes cold shader and file cache; 4.25 s warm.
- The fast-run max of 166.5 ms is a single early frame (shader compilation / first-use cost), not steady state.
- The UHD and CPU runs are reference only and are far too slow for real time.

## Transcribed reply text (what Haru "said", from the frame `text` fields)

- B580 fast: `" Hallo.  Ich habe es gut.  Ja, wie geht es dir  heute? "`
- B580 realtime: `" Mir gut, wie geht's dir?  Ja, ich  gut.  ich bin gut und wie geht's es?  ganz gut eigentlich.  Ichel "`
- UHD: `" Ach du  ach du. "`
- CPU: `" Hallo da h ich da  okay dann wie kann ich heute so saß ich da ich war dann da "`

Model-behaviour observation: with the synthetic SAPI voice, replies are short and sometimes degenerate. In an earlier manual `speech talk` run with `--persona/--situation/--speak-first` the reply was repetitive filler ("ja also ist so ein bisschen ..."). The full-duplex model reacts to the artificial voice; this is not a compute problem.

## T0.1 manual measurements (`speech talk`)

- `speech talk <model> --device Vulkan0 --language de --name Haru -o reply.wav <8 s question + 14 s silence>`: "Loaded haru-4b on Vulkan0 in 2.62 s"; 166 frames computed in 16.599 s, RTFx 1.33 (no warm-up, includes shader compile).
- 69 s file with `--persona/--situation/--speak-first`: 519 frames in 16.510 s, RTFx 4.19 (about 32 ms/frame), consistent with the p50 of about 35 ms above.

## Decision

GATE: PASS. The B580 computes a frame in a median of about 35 ms (p99 below 70 ms in the realtime run), i.e. about 3.8x faster than real time, with no lag growth. Continue with M1. The app should default to the first `kind: gpu` device from `speech devices` (not a hard-coded `VulkanN`) and warn on iGPU/CPU, which are 3 to 4 times slower than real time.
