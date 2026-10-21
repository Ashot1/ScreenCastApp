# Reproduce task 04

All commands start at `/Users/gojda/Projects/ScreenCastApp`. Only public FLEURS fixtures are suitable for the quality inspection described here. Do not put personal recordings or recognized personal text in evidence.

## Source and build

Requirements tested: Apple M1/16 GB, macOS 27.0 (26A428), Apple Swift 6.4, Python 3.13, ffmpeg. The helper is a test executable, not an implementation inside Ambient.

```sh
mkdir -p /private/tmp/ambient-04-parakeet
curl -L --fail https://api.github.com/repos/FluidInference/FluidAudio/tarball/v0.17.7 -o /private/tmp/ambient-04-parakeet/fluidaudio-v0.17.7.tar.gz
shasum -a 256 /private/tmp/ambient-04-parakeet/fluidaudio-v0.17.7.tar.gz
# Observed tar SHA256: 77467b4ebebbc29cf328758378cfc99af60a9a5b9983b0b6e0a2295b9409bc52
# Annotated tag 2437bf2a65dc070eabf24e339f9d5a9723a5a8bf -> commit 503b4bd1bbf7220882de39fe8ae6716aae4132da.
tar -xzf /private/tmp/ambient-04-parakeet/fluidaudio-v0.17.7.tar.gz -C /private/tmp/ambient-04-parakeet
patch -d /private/tmp/ambient-04-parakeet/FluidInference-FluidAudio-2437bf2 -p1 < tests/feasibility/04-parakeet/fluidaudio-build.patch
env CLANG_MODULE_CACHE_PATH=/private/tmp/ambient-04-parakeet/module-cache SWIFTPM_MODULECACHE_OVERRIDE=/private/tmp/ambient-04-parakeet/module-cache PARAKEET_FLUIDAUDIO_SOURCE=/private/tmp/ambient-04-parakeet/FluidInference-FluidAudio-2437bf2 swift build --package-path tests/feasibility/04-parakeet -c release --product LocalParakeet --jobs 2 --disable-sandbox --scratch-path /private/tmp/ambient-04-parakeet/helper-build --cache-path /private/tmp/ambient-04-parakeet/cache --config-path /private/tmp/ambient-04-parakeet/config --security-path /private/tmp/ambient-04-parakeet/security
```

Patch only removes the unused `NemoTextProcessing` target and binary declaration from the Swift 6.2 manifest. ASR and all other runtime source files remain unmodified. `traits: []` disables the TTS normalization module but SwiftPM still eagerly resolves/downloads its binary artifact. The patch avoids that 87 MB unrelated artifact in this experiment. It is not a change to the application package or a proposal to ship a patched library.

## Weights

This Mac already had `~/Library/Application Support/FluidAudio/Models/parakeet-tdt-0.6b-v3`. No weights were installed by this task. All 21 used files were checked against Hugging Face revision `7dd20fe6b1797d35f5e3307e8b1732d9a178edfe`; see `model-hashes.json`. LFS files use SHA256; ordinary files use Git blob OID (`sha1("blob " + byteLength + NUL + contents)`). The manifest also records SHA256 for every file.

On another Mac, download the exact files listed in `model-hashes.json` from:

`https://huggingface.co/FluidInference/parakeet-tdt-0.6b-v3-coreml/resolve/7dd20fe6b1797d35f5e3307e8b1732d9a178edfe/<file>`

Keep component directories intact and use their common parent as the helper's first argument. Required bundles: Preprocessor, Encoder, Decoder, JointDecisionv3 (`.mlmodelc`) and `parakeet_vocab.json`. No `downloadAndLoad`, mutable `main` or cloud fallback is used. Standard `.int8` mode in this release actually selects the 6-bit LUT-palettized `Encoder.mlmodelc`; it does not select `Encoder_v2`.

## Public audio

Download `dev.tsv` and `audio/dev.tar.gz` for each `ru_ru` and `en_us` from:

`https://huggingface.co/datasets/google/fleurs/resolve/70bb2e84b976b7e960aa89f1c648e09c59f894dd/data/<language>/<file>`

Save as `/private/tmp/ambient-04-parakeet/<language>-dev.tsv` and `<language>-dev.tar.gz`. The following prepares the same four inputs. This is public speech from human speakers, not TTS; long inputs concatenate five distinct utterances and are not continuous dictation.

```sh
python3 - <<'PY'
from pathlib import Path
import csv, tarfile, subprocess
base = Path('/private/tmp/ambient-04-parakeet')
for lang in ['ru_ru', 'en_us']:
    rows = list(csv.reader(open(base / f'{lang}-dev.tsv'), delimiter='\t'))
    selected, seen = [], set()
    for row in rows:
        if row[2] not in seen:
            seen.add(row[2]); selected.append(row)
        if len(selected) == 5:
            break
    folder = base / f'{lang}-fixtures'
    folder.mkdir(exist_ok=True)
    with tarfile.open(base / f'{lang}-dev.tar.gz') as archive:
        for row in selected:
            (folder / row[1]).write_bytes(archive.extractfile('dev/' + row[1]).read())
    for kind, clips in [('short', selected[:1]), ('long', selected)]:
        filelist = folder / f'{kind}.txt'
        filelist.write_text(''.join(f"file '{folder / row[1]}'\n" for row in clips))
        subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y',
                        '-f', 'concat', '-safe', '0', '-i', str(filelist),
                        '-ac', '1', '-ar', '16000', '-c:a', 'pcm_s16le',
                        str(base / f'{lang}-{kind}.wav')], check=True)
PY
ffmpeg -hide_banner -loglevel error -y -f lavfi -i anullsrc=r=16000:cl=mono -t 4 -c:a pcm_s16le /private/tmp/ambient-04-parakeet/silence.wav
printf 'invalid public input' > /private/tmp/ambient-04-parakeet/invalid.wav
```

Generated hashes, source filenames, durations and public references are in `public-fixtures.json`. Attribution: Google FLEURS / Conneau et al., dataset revision above, [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/); changes: selected clips, concatenation, mono PCM16/16 kHz conversion. Public source fixtures are reproducible inputs, separate from owned per-operation audio copies.

## Probe invocation

```sh
python3 tests/feasibility/04-parakeet/test_probe.py
sandbox-exec -p '(version 1)(allow default)(deny network*)' /usr/bin/curl -I --max-time 3 -sS https://huggingface.co
# Negative control: must fail under the no-network profile.
sandbox-exec -p '(version 1)(allow default)(deny network*)' python3 tests/feasibility/04-parakeet/probe.py --audio /private/tmp/ambient-04-parakeet/ru_ru-short.wav --metrics /private/tmp/ambient-04-parakeet/ru-short-metrics.json -- /private/tmp/ambient-04-parakeet/helper-build/release/LocalParakeet '/Users/gojda/Library/Application Support/FluidAudio/Models/parakeet-tdt-0.6b-v3' '{audio}' '{output}' '{ready}'
```

Repeat for both languages, both durations and silence. For invalid audio use `invalid.wav`. For execution failure replace the model directory with a nonexistent directory. For startup cancellation add `--cancel-after 0.05`; for active inference use the long file and `--after-ready --cancel-after 0.05`. The ready marker is created after models load, immediately before actual batch inference. A hard timeout of 180 s prevents an indefinite experiment. The owned worker receives SIGTERM, followed by SIGKILL after 250 ms if needed, then `wait4` confirms process exit before removing the owned scratch directory.

The supervisor sets worker TMPDIR/cwd for ordinary worker temporary files. On tested macOS, Foundation ignores TMPDIR; do not use that variable as proof of audio cleanup. The selected native helper instead sets `ASRConfig(streamingEnabled: false)`: the library reads the completed WAV into memory and batch-chunks longer input without creating its disk-backed raw derivative. This preserves long-file recognition but memory scales with audio duration (at least 64 kB/s of 16 kHz Float32, plus full-rate mono conversion, model and chunk buffers). The supervisor emits only metrics. It copies the complete public input to a private temporary directory before spawning, discards worker stdout/stderr, reads text only from the ephemeral JSON channel, then removes input, result and ready marker together. Public source fixtures remain unchanged. This is not a microphone recorder; task 10 must close recording before handing its owned audio to a worker.

## Main UI loop proxy and checks

```sh
swiftc -module-cache-path /private/tmp/ambient-04-parakeet/module-cache tests/feasibility/04-parakeet/ui-heartbeat.swift -o /private/tmp/ambient-04-parakeet/ui-heartbeat
/private/tmp/ambient-04-parakeet/ui-heartbeat 10
# Repeat concurrently with actual inference for the loaded measurement.
yarn verify
swift format lint --strict --configuration .swift-format tests/feasibility/04-parakeet/Package.swift tests/feasibility/04-parakeet/Sources/LocalParakeet/main.swift tests/feasibility/04-parakeet/ui-heartbeat.swift
```

The heartbeat is a hidden AppKit application's 60 Hz main-run-loop timer, with p95/max callback gaps and counts above 50 ms. It measures native event-loop scheduling, not rendered frames or human interaction in a finished Ambient UI. CPU time and peak RSS come from `wait4` for the actual worker; they exclude system CoreML/ANE services and total device memory. Wall time includes model loading and process startup. No UI quality or latency threshold was invented for the ticket.

Final selected-path evidence is `measurements.json`; `initial-cold-load.json` is the earlier short-file diagnostic with the same model loader, before the disk-backed branch was disabled. It records the failed Foundation temp ownership assumption explicitly. `public-quality.json` contains only public fixture references/hypotheses; word error rate uses minimum insertion/deletion/substitution distance after the documented normalization.

For the tighter UI workload, run the 20-second heartbeat while repeatedly launching the same supervisor command with `ru_ru-long.wav` until 20 seconds elapse; await each worker sequentially. The observed workload completed 21 operations. This avoids the idle tail of the separate 60-second whole-matrix observation. Preserve both series.
