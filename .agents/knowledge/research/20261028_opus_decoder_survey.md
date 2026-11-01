# Opus Decoder Survey for no_std ESP32 Firmware (WO-200)

- **Source**: research subagent [`b3d8d909`](conversation://b3d8d909-3464-4c40-8849-5e044e7afee6), 2026-10-28. The subagent was read-only, so the orchestrator transcribed this note from its report.
- **Network access during research**: read-only GETs to the crates.io API, static.crates.io and raw.githubusercontent.com.
- **Tools used**: `~/.espressif/tools/xtensa-esp-elf/esp-14.2.0_20260121/xtensa-esp-elf/bin/xtensa-esp32-elf-{nm,size,objdump,ar}`

## 1. Original `components/codecs/lib/libopus.a` (local command output)

- **Build type**: fixed-point. Version string is `"libopus unknown-fixed"`.
  - 131 objects, including `silk/fixed/*_FIX.c.obj`.
  - Float instructions appear only in `mlp.c.obj` (encoder analysis code).
  - `opus_decode_float` is absent.
- **Contents**: includes the encoder, multistream and projection code.
- **Size**:
  - Whole archive: text = 201,725 B.
  - Decoder-only subset (51 objects, filtered by name, approximate): text = 91,471 B.
- **Pseudostack**: `celt_decode_with_ec` does a process-global lazy `malloc(120000)` (`global_stack`/`scratch_ptr`) and never frees it.
  - The library is non-thread-safe, so only one decoder can be active at a time.
  - Largest stack frames: 1,856 B and 768 B.
- **Per-decoder RAM** for this binary: Information unavailable.
  - Espressif reports 26.6 KB heap for its own build on ESP32-S3.
- **Undefined imports**: `abort`, `__ashldi3`, `__ashrdi3`, `__divdi3`, `__extendsfdf2`, `__fixdfsi`, `floor`, `fprintf`, `free`, `__getreent`, `malloc`, `memcpy`, `__memcpy_chk`, `memmove`, `memset`, `__memset_chk`, `__stack_chk_fail`, `__stack_chk_guard`.
  - The `__stack_chk_*` imports mean the archive was built with stack protection.
  - The `__mem*_chk` imports mean it was built with `_FORTIFY_SOURCE`.
- **Imports a bare-metal esp-hal image already provides**:
  - `malloc`, `free`: esp-alloc 0.11.0 `compat` feature (on by default).
  - `mem*` and libgcc helpers: ROM linker scripts.
  - `abort`: `esp32.rom.redefined.ld`.
  - `__getreent`: `esp32.rom.syscalls.ld`.
  - `__stack_chk_fail`: esp-hal `soc/mod.rs:243`.
  - `__stack_chk_guard`: esp-hal `stack.x:9`.
- **Imports that must be stubbed**: `fprintf`, `__memcpy_chk`, `__memset_chk`, plus `floor` if `mlp.c` gets linked.
- **Original C wrapper** `components/squeezelite/opus.c`:
  - Calls only `opus_decoder_create`, `opus_decode` (int16, `MAX_OPUS_FRAMES` 5760) and `opus_decoder_destroy`.
  - Ogg demuxing goes through libogg.
- **Full libopus source tree in the repo**: `components/spotify/cspot/bell/external/opus`.
  - CMake option `OPUS_FIXED_POINT` defaults to OFF.
  - Version unverified.
  - About 38.8k LOC of portable C, roughly 20k after filtering out encoder files.

## 2. Pure-Rust Opus decoders

| Crate | Version / date | License | Modes | no_std | Arithmetic | Conformance | LOC | Notes |
|---|---|---|---|---|---|---|---|---|
| opus-rs ([repo](https://github.com/restsend/opus-rs)) | 0.1.40 / 2026-10-25 | BSD-3 | CELT, SILK, Hybrid, PLC, FEC | **Yes**, no alloc needed, `libm` feature | float, "close rather than bit-exact" | Information unavailable | 36,434 | Xtensa not among listed targets; ~175 KB inline decoder state in no_std mode; 285 `unsafe`; very young |
| ropus ([repo](https://github.com/0x4D44/ropus)) | 0.12.18 / 2026-05-11 | BSD-3 | SILK, CELT, hybrid, FEC, PLC (+DNN PLC) | No (std use is mostly core/alloc-equivalent) | fixed-point | claims bit-exact vs C reference | 96,070 | 776 KB crate (weights, watch flash usage); 12 `unsafe` |
| opus-decoder / Rusopus ([repo](https://github.com/TadeuszWolfGang/Rusopus)) | 0.1.1 / 2026-03-12 | MIT/Apache-2.0 | CELT, SILK, Hybrid, PLC | No | i16 / f32 output | README: 12/12 RFC 8251 vectors | 14,792 | decoder-only, `forbid(unsafe_code)` |
| rusty-opus ([repo](https://github.com/Remade-With-Rust/rusty-opus)) | 1.0.2 / 2026-10-17 | BSD-3 | full enc/dec | No | float + SIMD | README: bit-exact on 12 vectors | 40,877 | heavy use of std::arch/sync/thread |
| unsafe-libopus ([repo](https://github.com/DCNick3/unsafe-libopus)) | 0.2.0 / 2024-11-21 | BSD-3 | libopus 1.3.1 (c2rust) | No | float | IETF vectors | 48,859 | 589 `unsafe` |

FFI-only crates (not pure Rust): `opus-codec` 0.2.0, `magnum-opus` 0.3.2.

## 3. CPU load

- **ESP32-S3R8, 48 kHz stereo**: 5.86 % CPU and 26.6 KB heap.
  - Source: [esp_audio_codec README](https://raw.githubusercontent.com/espressif/esp-adf-libs/master/esp_audio_codec/README.md).
  - The test input was an encoded sine wave.
- **Original ESP32 (LX6)**: Information unavailable.

## 4. Ranked recommendation (effort figures are the subagent's estimates)

1. **(c) FFI to the existing `libopus.a`** (≈2–4 days, medium-low risk).
   - It is the same binary the original firmware shipped, and only 3 stubs are needed.
   - Risks:
     - Unknown version, so CVEs can't be tracked.
     - The global 120 KB pseudostack must live in PSRAM.
     - GCC and LLVM objects have never been link-tested together.
     - LX6 CPU load is unmeasured.
   - **Variant (c′)**: rebuild from the bell source tree with `OPUS_FIXED_POINT=ON`, decoder only, known flags (+1–2 days). This fixes provenance.
2. **(a) Port a pure-Rust crate** (≈4–8 days plus 1–2 days of LX6 performance testing, medium-high risk).
   - Candidates: opus-rs (already no_std), ropus (fixed-point, easiest port), opus-decoder (best conformance and safety).
3. **(d) Leave Opus transcoding to LMS** (≈0–0.5 day, low risk). Opus would not play natively on the device.
4. **(b) Our own c2rust translation** (≈2–4 weeks, high risk). It has no advantage over (c′).

## 5. Unverified points

- The decoder-only link closure was inferred from `nm`, not from a test link.
- There are no LX6 CPU measurements.
- The version of the bundled libopus source is unknown.
