# Work Order: WO-200 — S4 Prep: Opus Decoder Strategy Research (Read-Only)

- **Work Order ID**: `WO-200`
- **Timestamp**: `2026-10-28T12:32:00-04:00`
- **Assigned Subagent Role**: `Research Specialist (Opus Decoder Survey)`
- **Plan**: `/home/console-admin/.gemini/antigravity/brain/6b0546bc-7be8-4fb1-bca2-7848797b09ed/implementation_plan.md` (decision D2)
- **Status**: `[IN PROGRESS]`

## 1. Objective
Find out whether Ogg Opus decoding (CELT + SILK + hybrid, 48 kHz) can run in the bare-metal `esp-hal` `no_std + alloc` firmware on an ESP32 (Xtensa LX6, 240 MHz, single-precision FPU, 4 MB PSRAM). **Read-only: modify no files except the deliverable below.**

## 2. Questions
1. Which pure-Rust Opus decoders exist (crates.io and GitHub)? For each: version, last release date, license, feature coverage (CELT / SILK / hybrid / PLC), `no_std` support, float vs fixed-point, conformance-test status (RFC 6716 / RFC 8251 test vectors), and LOC. Cite URLs.
2. Is there a fixed-point C reference path (`libopus` built with `--enable-fixed-point`) that could be translated with c2rust, or linked through FFI? How big is the static library, and how much RAM per decoder? The original firmware's `/home/console-admin/Github/squeezelite-esp32/components/codecs/lib/libopus.a` and `components/squeezelite/opus.c` may be inspected (read-only).
3. If we link the C library via FFI, which symbols does `libopus.a` import (`malloc`, `free`, `memcpy`, libm, etc.; use `xtensa-esp32-elf-nm -u` if available)? Can a bare-metal `esp-hal` image provide them?
4. Published ESP32 CPU-load figures for Opus decoding at 48 kHz stereo.
5. Recommendation, ranked: (a) pure-Rust crate (vendor + `no_std` port), (b) c2rust translation, (c) FFI to `libopus.a`, (d) leave Opus transcoding on LMS. Give effort estimate and risk for each.

## 3. Deliverables
- Write `/home/console-admin/Github/squeezelite-esp32/.agents/knowledge/research/20261028_opus_decoder_survey.md` and append a short Fulfillment Summary below.
- Do not state anything as fact without a cited source or command output (`no-guessing.md`). If something can't be verified, write "Information unavailable."

---

## Fulfillment Section
- **Status**: `[EXECUTED & VERIFIED]` (research complete). The subagent `b3d8d909` was read-only and could not write files, so the orchestrator transcribed its report on 2026-10-28T12:38-04:00.
- **Deliverable**: [20261028_opus_decoder_survey.md](file:///home/console-admin/Github/squeezelite-esp32/.agents/knowledge/research/20261028_opus_decoder_survey.md)
- **Ranked recommendation**:
  1. (c/c′) FFI to fixed-point libopus. Prefer (c′), a decoder-only rebuild from `components/spotify/cspot/bell/external/opus`.
  2. (a) Pure-Rust port (opus-rs / ropus / opus-decoder).
  3. (d) Leave Opus transcoding on LMS.
  4. (b) c2rust.
- **Open items**: no LX6 CPU data; the decoder-only link was never test-linked; libopus version unknown. Decision D2 is pending with the user.
