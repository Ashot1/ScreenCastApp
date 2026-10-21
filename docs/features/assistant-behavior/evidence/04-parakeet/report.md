# 04 — Parakeet feasibility

**Result: DONE. G3: PASS for local use on the tested Mac.** Date: 2026-10-18. This is an executed feasibility package, not a completed microphone/UI feature. Task 10 may use this local path after its other dependency is ready.

Selected path: **FluidAudio 0.17.7 → CoreML Parakeet TDT 0.6B v3 → completed WAV → editable text**, in a separate native worker. The helper uses **`ASRConfig(streamingEnabled: false)`**: long completed files are still batch-chunked, while intermediate audio stays in memory. No microphone, live transcription, cloud route or application-runtime mock was added.

## Acceptance criteria

| Ticket criterion | Result | Actual evidence / boundary |
| --- | --- | --- |
| Runtime, exact weights, format, licenses, redistribution conditions and hardware | **PASS** | Release/commit and all 21 used weight files pinned and verified. CoreML bundles load on this M1. Runtime Apache 2.0; original/model metadata CC BY 4.0. Conversion card has an inconsistent Apache footer: preserve original attribution and CC BY obligations, never describe weights as Apache-only. No weights redistributed. [Sources](sources.md), [hashes](model-hashes.json). |
| Real RU/EN speech, short/long, silence, invalid input, execution error; measured quality | **PASS** | Public human FLEURS speech: 8.28/55.20 s RU, 11.46/47.80 s EN. Silence returns empty text; invalid WAV and missing models return error/exit 1. Four exact hypotheses compared with references; WER 0–8.77%. [Fixtures](public-fixtures.json), [quality](public-quality.json), [13 cases](measurements.json). |
| No transcription during recording, audio cloud upload or cloud fallback | **PASS** | Only closed, completed files are handed to worker; no capture code exists. All actual model probes run under `sandbox-exec` with `deny network*`; localhost connect rejected with errno 1 and HTTPS request rejected. Loader is `loadLocal`, no downloader/fallback. [Privacy audit](privacy-audit.json). |
| Time, CPU/memory and influence on UI measured on specified Mac | **PASS** | Actual process wall/CPU time and peak RSS, plus native AppKit main-loop timer. Warm repeated full operation 0.679–1.115 s, peak process RSS up to 109.48 MiB in the main matrix. First cold CoreML preparation took 140.42 s. UI proxy and its limits below. |
| Cancellation/stop releases worker only after real completion | **PASS** | Actual model stopped before loading and during long-file processing: exit -15, reaped by `wait4`; cancel-to-exit 11.23/12.83 ms. Separate real-child test exercises SIGTERM refusal → SIGKILL and a wait/signal race. No cancellation acknowledgement is treated as completion. |
| Audio/temporary derivatives removed after success, error, cancellation; no personal text in normal logs | **PASS** | All 13 owned session directories removed after confirmed exit; no new `fluidaudio-streaming-*.raw` files globally. Found and removed the disk-backed-audio mechanism rather than trusting TMPDIR. Public quality runs emitted zero stdout/stderr bytes; audit of 2,504 LocalParakeet log messages found no known recognized phrases. No personal audio/text used. |
| Reproducible tests and suitability decision saved; no unverified compatibility/cancellation waiver | **PASS** | [Reproduce](reproduce.md), real Swift helper, supervisor, six lifecycle regression tests, build/check logs and exact source/weight/fixture versions saved. Suitable for local completed-recording replies with user text review; limitations below remain explicit. |

## Tested configuration

- MacBook Pro `MacBookPro17,1`, Apple M1, 8 CPU cores (4 performance + 4 efficiency), 16 GB RAM; macOS 27.0 `26A428`, arm64. Hardware identifiers/serials are not stored.
- Apple Swift 6.4 (`swiftlang-6.4.0.34.1`, clang `2100.3.34.1`), macOS SDK 27.0. Python 3.13.2, ffmpeg 8.0.1; Node 22.23.1, Yarn 1.22.22, Rust 1.99.0 for foundation checks.
- FluidAudio v0.17.7, commit `503b4bd1bbf7220882de39fe8ae6716aae4132da`; source archive SHA256 `77467b4ebebbc29cf328758378cfc99af60a9a5b9983b0b6e0a2295b9409bc52`.
- Converted weights `FluidInference/parakeet-tdt-0.6b-v3-coreml`, revision `7dd20fe6b1797d35f5e3307e8b1732d9a178edfe`; 21/21 files match pinned HF SHA256/Git blob OIDs, total 483,105,645 bytes. Existing user FluidAudio model cache was read; no model weights installed or replaced.
- Required format: `Preprocessor.mlmodelc`, `Encoder.mlmodelc`, `Decoder.mlmodelc`, `JointDecisionv3.mlmodelc`, `parakeet_vocab.json`. Runtime `.int8` selects the standard **6-bit LUT-palettized** Encoder, not Encoder_v2. CPU + Neural Engine defaults; preprocessor CPU-only. No GPU path selected.

## Results

Selected helper, repeated warm launches; each operation includes a fresh worker and model loading. Time excludes creation of the public input fixture but includes its owned session copy.

| Case | Audio | Full operation | Inference | Worker CPU | Peak worker RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| RU short, repeat | 8.28 s | 0.706 s | 0.243 s | 0.517 CPU-s | 76.81 MiB |
| RU long, repeat | 55.20 s | 1.115 s | 0.807 s | 1.747 CPU-s | 105.47 MiB |
| EN short, repeat | 11.46 s | 0.679 s | 0.286 s | 0.538 CPU-s | 74.05 MiB |
| EN long, repeat | 47.80 s | 0.852 s | 0.579 s | 1.620 CPU-s | 105.52 MiB |

CPU seconds can exceed wall time because chunk work is parallel. RSS/CPU cover the actual worker, excluding system CoreML/ANE services and total device memory. The machine was an active workstation with other work; these are observations, not an isolated statistical benchmark.

The first-ever observed load took **140.419 s**, of which inference was **1.409 s**, with 422.63 MiB peak worker RSS. Sampling located the delay in CoreML on-device compilation while loading the encoder. This initial short-file diagnostic preceded disabling disk-backed audio; the loader and short-audio execution path are the same. System caches were not erased to manufacture another cold run. Following preparation, the first selected-path RU operation took 3.142 s; repeated operations are in the table. **Do not promise immediate first recognition before model preparation.**

Quality uses Unicode word tokens, lowercase, `ё→е`, punctuation ignored; numbers are not normalized between digits and words. Minimum word insertion/deletion/substitution distance:

| Public fixture | Word edits / reference words | WER |
| --- | ---: | ---: |
| RU short | 0 / 7 | 0% |
| RU long | 7 / 89 | 7.87% |
| EN short | 1 / 24 | 4.17% |
| EN long | 10 / 114 | 8.77% |

There are real name errors: RU “Рик” → “Эрик”, “Крюгера” → “Кругера”; short EN “Manta” → “Monta”, long EN loses part of “Uthappa”. Some other edits reflect digit/word formatting. This supports the agreed editable draft and explicit confirmation; it does not support automatic sending. The corpus is tiny, public, and not established as held out from model training. Long inputs concatenate five distinct human utterances; continuous personal dictation, noise and accents were not comprehensively assessed. No accuracy threshold was invented after seeing the result.

Native UI proxy: a separate hidden AppKit application's 60 Hz main-run-loop timer. No windows were shown and no focus/input was changed.

| Observation | Duration / samples | p95 callback gap | Maximum gap | Gaps >50 ms |
| --- | --- | ---: | ---: | ---: |
| Baseline | 10 s / 572 | 25.31 ms | 97.81 ms | 3 |
| Whole matrix, including idle tail | 60 s / 3,402 | 22.92 ms | 264.36 ms | 30 |
| Continuous sequential inference | 20 s / 1,174 | 19.74 ms | 137.98 ms | 3 |

Continuous workload completed **21** genuine long-RU operations; all succeeded, were reaped and cleaned. The main loop continued running, but occasional long gaps remain. These measurements do not establish smooth rendering, actual Ambient input latency, or a causal before/after difference under changing workstation load. Task 10 still owns real microphone/UI flows and UI acceptance. Both favorable and unfavorable timer samples are retained.

## Cancellation, cleanup and the corrected finding

`wait4` is the sole normal owner of process completion and resource usage. Signals use `os.kill` for the owned worker; `Popen.terminate/kill` were avoided because their internal polling can reap the child and race with `wait4`. SIGKILL follows an uncompleted SIGTERM after 250 ms, then actual exit is awaited. Exception cleanup also waits before deleting owned audio.

For actual Parakeet: startup cancellation finished in 0.067 s overall; processing cancellation in 0.583 s, after the post-load ready marker and 0.3 s of processing. Both exited by SIGTERM and returned no usable text. A real Python test child ignoring SIGTERM proves the SIGKILL path, not model accuracy.

Simplicity review found that FluidAudio normally switches files longer than 30 s to a `.raw` file in `FileManager.default.temporaryDirectory`. **Actual macOS Foundation ignored TMPDIR**, despite the Python worker honoring it. That assumption failed and was not accepted as evidence. The selected native configuration disables that disk-backed branch entirely and uses the library's existing in-memory converter/long-file chunker. Final long-file/error/cancellation matrix found zero new global raw files and an empty owned session root.

Tradeoff: memory grows with recording length. The 16 kHz float array alone is about 64 kB/s; full-rate mono conversion buffers, models and chunks add more. The measured speech coverage ends at 55.2 s; no constant-memory or arbitrary-duration claim is made, and no new product duration limit was imposed.

The result file contains text only inside the private operation directory, read after success and then deleted. Public source fixtures remain reproducible inputs, separate from owned copies. Ordinary helper success stdout/stderr are empty; framework diagnostic threshold is set before inference, error output is a stable non-content code. Log audit proves absence of the known public test phrases in this run, not a blanket audit of future application logs.

## Build, checks and specialists

- Real release build **PASS**, first build 776.66 s; selected-path helper rebuild **PASS**, 8.59 s; final delivery rebuild **PASS**, 3.11 s ([log](delivery-build.log)). A debug control build was cancelled once release completed; no debug performance is reported.
- SwiftPM eagerly downloaded the unrelated optional Nemo TTS artifact even with its trait disabled. A saved patch removes only that unused binary target/dependency from the temporary upstream manifest. No ASR/runtime source was modified, no shared project manifest changed. The application has not adopted a patched dependency. See [reproduction](reproduce.md) and `fluidaudio-build.patch`.
- Initial upstream compilation emitted **37 source warning diagnostics**, including an unused Cohere-backend pointer warning and Swift diagnostics. They were not suppressed or “fixed” in third-party code. The tested Parakeet path passed real execution; this is not a claim that every FluidAudio backend is warning-free. [Build log](build.log).
- **Six lifecycle tests PASS**: success/privacy, partial-output error cleanup, malformed worker JSON, operation-owned Python temp files, SIGTERM-resistant worker, exit-between-wait-and-signal race. Relevant failures were observed before corrections; actual native probes are separate from these test-only workers. [Test log](lifecycle-tests.log).
- `yarn verify` **PASS**: agents check, 8 tooling tests, Rust check/fmt/Clippy/tests, Swift strict build and format lint. Core currently has zero behavior tests. Standard sandbox first blocked nested SwiftPM sandbox; the complete command succeeded through the authorized unsandboxed execution. Final standalone Swift strict format lint and Python compilation also passed. [Foundation log](foundation-verify.log).
- Called **Codebase Scout**, **Pattern Librarian**, **External Researcher**, **Code Simplicity Reviewer**, with targeted rechecks of found lifecycle/temp defects. Adopted native `loadLocal`, exact model inventory, real human fixtures, sole wait owner and elimination of disk derivatives. Review of corrected sections has no remaining findings. No new translation keys, provider registry, named domain/transport/FFI declarations or generic implementation workers; their mechanics were not triggered. No user-requested visual inspection was performed.

No production runtime, shared manifests, specification, decisions, README or other workers' files changed. No Git initialization, worktree, commit, PR or publication. No paid API calls or key inspection. Windows, a finished Ambient UI, microphone permissions/capture and final model packaging are not claimed tested.

## Handoff to task 10

Use the tested v3 bundles and local-only loader; run native inference outside the UI process. Close recording before starting; keep decoded text an editable draft and send only after confirmation. Retain worker ownership until actual exit, then remove owned audio/result. Preserve the in-memory completed-file path unless a separately verified owned disk strategy replaces it. Handle one-time model preparation and retain license/source attribution; do not relabel weights Apache-only.

**Open blockers for this local G3 package: none.** Distribution metadata inconsistency, long-duration memory scaling, first-load preparation cost and real UI/microphone acceptance remain explicit integration constraints, not waived measurements.
