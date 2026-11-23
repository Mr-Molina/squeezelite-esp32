# Infrastructure Invariants & Mechanical Guardrails

The following mechanical invariants are binding safety rules enforced across sessions and workflows:

> **Numbering convention**: Global invariants use numbers **1-99** and are defined only in this canonical file (`S:\Box\Ideas\System\Agents\INFRASTRUCTURE_INVARIANTS.md`; the seed template `C:\Users\jmolina\.gemini\config\templates\INFRASTRUCTURE_INVARIANTS.md` is kept byte-identical). Project-local invariants appended to a workspace's `.agents/INFRASTRUCTURE_INVARIANTS.md` **MUST** use the `P-<n>` prefix (e.g. `P-1`, `P-2`) and MUST NOT reuse global numbers. Retired global numbers are marked merged/retired, never renumbered.

## Invariant 13: Debug Artifact Cleanup
- **Rule**: Prior to completing a session handoff, all temporary debug scripts, scratch logs, and transient build artifacts must be sanitized or removed.

## Invariant 14: Three Strikes Rule for Troubleshooting
- **Rule**: Maximum of **three attempts** allowed to solve a specific technical error or system failure. If the third attempt fails, all modification attempts MUST stop immediately and a Deep Research Request must be compiled.

## Invariant 24: Credential Leak Scan
- **Rule**: No plain-text API keys, passwords, private tokens, or secrets may be committed or persisted in public artifacts. A secret scan must run prior to session closing.

## Invariant 28: Mechanical Mitigations for Recurring Failures
- **Rule**: Any failure identified as RECURRING must be addressed using mechanical mitigations (strict MUST/MUST NOT directives or automated scripts), not advisory comments.

## Invariant 29: Script Verification Requirement
- **Rule**: After creating or modifying helper scripts (such as Python cleaner or audit scripts), the agent MUST run a syntax check/CLI test execution before marking the task complete.

## Invariant 30: Tool Name Schema Integrity
- **Rule**: Imported skills MUST be validated against active runtime tool declarations (`search_web`, `run_command`, `invoke_subagent`, etc.) before integration into global config.

## Invariant 31: Python Code Generation Safety
- **Rule**: The agent MUST NOT embed Python code containing quotes or regex patterns inside PowerShell `Set-Content @"..."@` heredocs. The agent MUST use native file tools or clean Python script writes to avoid string quote escaping corruption.

## Invariant 32: Strict Verification Exit Criteria
- **Rule**: When verifying script execution per Invariant 29, the agent MUST NOT accept CLI output with non-zero exit codes or usage/help messages as verification. The agent MUST execute scripts against a valid test input file and verify an exit code 0 result.

## Invariant 33: Topology-Based Delegation Threshold (Single-Agent Cascade Default)
- **Rule**: Keep strongly coupled multi-file code edits and sequential debugging inside a single 1M-context Gemini 4 thread with **no artificial tool-call cutoff** (replacing the retired 8-operation cap). Delegate via `run_workflow`, `invoke_subagent`, or `swarm` when tasks: (1) fan out over **3 or more independent targets**, (2) produce noisy exploration or log output **over 20KB**, or (3) require independent **Challenger verification**. `parse_transcript.py` enforces a runaway loop warning threshold (`GLB020_WAVE_LIMIT = 25`), excluding exempt calls: (a) reading a required `SKILL.md` (and `references/*.md`); (b) orchestration, messaging, planning, and live UI progress calls (`invoke_subagent`, `send_message`, `schedule`, `manage_subagents`, `manage_task`, `swarm`, `run_workflow`, `update_focus`, `update_steps`, `ask_question`, `exit_plan_mode`, `enter_plan_mode`, `task_boundary`); (c) artifact directory writes; and (d) `/goodbye` preflight/ledger sync (`goodbye_preflight.py` + `sessions.md`), SDD controller scripts (`sdd-workspace`, `task-brief`, `review-package`), `parse_transcript.py --heartbeat <transcript_path>`, and TokenEconomizer CLI (`te status` / `te repomap` / `te repomap refresh`).

## Invariant 34: Orchestrator Delegation Mandate, Gemini 4 Tiering & Micro-Subagent Fan-Out Gate (PM-2026-1102-007 / GLB-020)
- **Rule**: When topology triggers delegation (3+ independent targets, >20KB noisy exploration/logs, or independent Challenger verification), the Orchestrator delegates via `invoke_subagent`, `swarm`, or `run_workflow` using valid `TypeName` values (`TypeName: "self"` for execution/shell/network tasks or `TypeName: "research"` for read-only exploration) paired with descriptive `Role` titles (`Role: "Discovery Scout"`, `Role: "Research Specialist"`, `Role: "Reconnaissance Lookout"`, `Role: "Cleanup Specialist"`). Reactive wakeup is the default after launching subagents or background tasks (end the turn without polling; optional `60s` `schedule` watchdogs are reserved for `>3m` external waits).
- **Gemini 4 Tiering Matrix & Scope Slicing Gate (`PM-2026-1102-007` / `GLB-020`)**: Subagents MUST be scoped to at most `<= 8` target files and `<= 8` numbered task items (enforced mechanically by `pre_tool_guard.py`), using parallel fan-out rather than monolithic checklists, defaulting to `<= 12–15 tool steps / <= 90s` (`<= 25 steps / <= 180s` hard ceiling), forbidding `send_message` wrap-ups to subagents that have already returned their report, and selecting models via the Gemini 4 tiering matrix: `carbon` and `argon` for architecture and hard RCA, `neon` as fast frontier coding workhorse, `hydrogen` for iterative coding and subagents, and `flash` / `flash_lite` for mechanical fan-outs (`inherit` default when matching parent).
- **Pre-Scout Repomap Gate**: Consult `te repomap --cwd <workspace>` before spawning `Discovery Scout` subagents in Git repositories where TokenEconomizer (`te`) is installed, refreshing first (`te repomap refresh`) if the repomap is stale.

## Invariant 35: Subagent Scope and Reporting
- **Rule**: Subagents operate within defined scope and must report findings back to the Orchestrator.

## Invariant 36: Invariant and Docstring Preservation
- **Rule**: Subagents must preserve existing codebase invariants and docstrings unless explicitly ordered to mutate them.

## Invariant 37: Subagent Command Exit Verification
- **Rule**: Subagents must verify all command exit codes before reporting task completion.

## Invariant 38: Three Strikes Deep Research Transition — Merged into Invariant 14 (Three Strikes)
- **Status**: Merged into Invariant 14 (Three Strikes). Number retained, not reused. Original text, kept for reference: On the 3rd failed attempt to resolve a technical error or on user third-strike override, agents MUST halt all code mutations, compile all environment diagnostics, hypotheses, and logs into a structured Deep Research Request, and await verified research findings before resuming.

## Invariant 39: Evidence Before Assertions (Anti-Guessing)
- **Rule**: Agents MUST cite specific evidence (command output, API response, log line) before stating any root cause. Agents MUST NOT present a hypothesis as a conclusion or propose a fix in the same message as discovering an unverified issue. Once the root cause is proven with cited evidence, or the user explicitly asked for a fix, evidence and fix may appear together, with the evidence cited first.

## Invariant 40: Live Ground Truth Requirement (Anti-Stale Information)
- **Rule**: When evaluating active infrastructure state, switch port assignments, link health, or online status, agents MUST query live infrastructure (SSH, API, CLI, SNMP) rather than relying on static documentation or legacy CSV export dumps.

## Invariant 41: Anti-Simulated Tool Verification Gate
- **Rule**: Agents MUST NOT assert or imply browser rendering, UI interactions, or tool executions occurred unless backed by actual tool calls with live responses or captured image artifacts. If a tool fails to attach, agents MUST immediately notify the user.

## Invariant 42: Staged Deployment & Manual Upload Gate
- **Rule**: When deploying hardware configurations requiring manual or external staging (FTP, TFTP, file servers), file generation and device restart commands MUST be strictly decoupled into separate phases with an explicit user confirmation gate between them.

## Invariant 43: Ground Truth Entity & Acronym Verification
- **Rule**: Agents MUST verify institutional names, client identities, organization titles, and domain acronym expansions against authoritative repository documentation or device backups before scaffolding UI templates or configuration copy.

## Invariant 50: Production 3D Scene Hygiene & Diagnostic Quarantine
- **Rule**: Diagnostic rendering instrumentation, debug cameras, and temporary inspection shaders MUST be isolated in non-production scenes or strictly cleaned up prior to completion. Armature or mesh resets MUST purge obsolete datablocks so user-facing 3D deliverables remain free of ghost bones, wireframe pyramids, or diagnostic contamination.

## Invariant 51: Windows Interactive Desktop `WinSta0\Default` Routing & Physical HWND Visibility
- **Rule**: When launching interactive Windows GUI applications for user visibility, agents MUST route child processes explicitly to `WinSta0\Default` (`STARTUPINFO.lpDesktop = "WinSta0\\Default"`) and verify physical window visibility (`IsWindowVisible(hwnd)` and valid non-zero `GetWindowRect` coordinates on `WinSta0\Default`) before claiming the window is open on screen.

## Invariant 52: Fail-Safe Win32 Window Management & Process PID Scoping
- **Rule**: Win32 window enumeration and state mutation calls (`EnumWindows`, `ShowWindow`, `SetForegroundWindow`, `BringWindowToTop`) MUST strictly validate a non-null `target_pid > 0` (or explicit application window class), exclude OS system/shell window classes (`Progman`, `WorkerW`, `Shell_TrayWnd`, `IME`, `ApplicationFrameWindow`, etc.), and abort if matching window count exceeds the safety bound (5).

## Invariant 53: PowerShell Inline Python Escaping & Syntax Hardening
- **Rule**: Agents executing commands in Windows PowerShell (`pwsh`) MUST NOT pass complex inline Python (`python -c` with f-strings, nested quotes, semicolons, or multi-line statements) or bash-style `\"` quote escaping. Multi-statement or string-formatting Python logic MUST be written to a `.py` file and executed directly.

## Invariant 65: Single-Target Troubleshooting Isolation & Post-Completion Scope Boundary Gate (GLB-012 / PM-2026-1026-007)
- **Rule**: When debugging an individual failing component, endpoint, or hardware unit alongside working components, agents MUST isolate modifications strictly to the failing target, use working components as read-only reference templates, and NEVER modify files the user confirmed are working without explicit user consent. Furthermore, when an assigned task or artifact update is 100% complete and zero open decisions remain within the user's requested task scope, agents MUST state completion cleanly and stop—agents MUST NOT solicit, pitch, or offer to execute follow-up remediation work for external conversations, historical logs, or unrelated components that were only inspected as read-only forensic evidence (`PM-2026-1026-007`), and MUST NOT invent artificial `"Open Next-Step Decisions"` or out-of-scope follow-up menus after a task is finished. Full rule: `C:\Users\jmolina\.gemini\config\rules\single-target-troubleshooting.md`.

## Invariant 66: Real-Time User Narration, Output Hygiene & Anti-Silent Tool Chaining Gate
- **Rule**: Every tool-calling turn on the Orchestrator main thread MUST include a concise 1–2 sentence user-visible status update explaining the action being taken (including Turn 1 of `/wakeup` and every subsequent tool turn). Agents MUST NOT execute silent back-to-back tool-calling turns with empty user-facing text, and MUST NEVER emit internal chain-of-thought deliberation, meta-commentary, or unverified suffix tokens into the user-facing response stream.

## Invariant 67: Embedded Appliance Boot/GPT, Wi-Fi, Klipper Motion & Vendor IPC Daemon Safety
- **Rule**: On embedded SoC appliances (e.g. Creality K1 / Ingenic X2000E, Broadcom Wi-Fi), agents MUST NOT write raw MBR/Primary/Backup GPT sectors (`dd of=/dev/mmcblk*`, `sgdisk`, `sfdisk`, `parted`, `fdisk`, raw `r+b` opens). Factory GPT geometry warnings are benign and MUST NOT be "repaired". Agents MUST NOT disable, strip execute bits from, move, or comment out proprietary vendor boot/IPC daemons (`app-server`, `upgrade-server`, `audio-server`, `display-server`, `master-server`, `log_main`) without tracing the full boot/IPC chain first.
- Before mutating `/etc/init.d/*`, mount helpers, Wi-Fi auth/regulatory config, or network config on a remote headless appliance, agents MUST arm a host-side Dead-Man's Rollback Watchdog and verify the change with a cold service restart and a guarded cold reboot. Warm runtime state alone is not proof. On Klipper/CoreXY 3D printers, agents MUST NOT alter sensorless homing (`HOME_CUR` / `driver_SGTHRS`) without full-stroke `G28` verification, MUST NOT zero out `[prtouch_v2] noz_ex_com` when probing below printing temperature, MUST split G-code lines on `;` before injecting parameters, and MUST NOT run chamber exhaust fans at high speed during `ASA`/`ABS`/`PC`/`PA` prints. Full rule: `C:\Users\jmolina\.gemini\config\rules\embedded-boot-safety.md` §1-5, enforced mechanically by `pre_tool_guard.py`.

## Invariant 68: Bounded Subagent Lifecycle (<= 25 Steps / 180s), Reactive Wakeup Default, Optional 60s Heartbeat & Scope Slicing Gate (PAT-2026-1014-001 / GLB-017 / PM-2026-1026-001 / PM-2026-1026-008 / PM-2026-1102-007)
- **Rule**: Every `invoke_subagent` prompt MUST bound the subagent to at most **25 tool steps or 3 minutes (`180s`)** before reporting (enforced mechanically by `pre_tool_guard.py` on `invoke_subagent`), defaulting to `<= 12–15 tool steps / <= 90s` for micro-subagents (`<= 25 steps / <= 180s` hard ceiling) with Gemini 4 tiering (`carbon` / `argon` for architecture and hard RCA, `neon` fast frontier coding workhorse, `hydrogen` iterative coding and subagents, `flash` / `flash_lite` mechanical fan-outs; `inherit` default when matching parent). Prompts MUST scope subagents to at most `<= 8` target files and `<= 8` numbered task items under the **Atomic Micro-Subagent Fan-Out & Scope Slicing Gate (`PM-2026-1102-007` / `GLB-020`)**, include `Parent Conversation ID: <id>` whenever mid-run heartbeats are needed, and forbid open-ended rabbit holes or redundant `send_message` wrap-ups to subagents that have already returned their report. **Reactive wakeup is the default**: after launching subagents or background tasks, the Orchestrator stops calling tools and waits for reactive wakeup rather than polling; arming a `60s` `schedule(DurationSeconds=60, TimerCondition="any")` watchdog timer (at most one active timer after checking `manage_task(action="list")`) is **optional** and reserved for `>3m` external waits. When an optional `60s` watchdog timer fires and a subagent is still running, the Orchestrator MUST run `python 'C:\Users\jmolina\.gemini\config\skills\agent-behavior-auditor\scripts\parse_transcript.py' --heartbeat <subagent_transcript_path>` (exempt from the runaway loop wave threshold) alongside `manage_subagents(action="list")` to emit concrete step/tool telemetry to the user—while raw `read_file`/`view_file` on `transcript.jsonl` remains strictly forbidden (`GLB-017`). If the subagent has exceeded `25 steps` or `180s`, the Orchestrator MUST intervene via `send_message` or kill/replace it and clean up dead partial scripts in `scratch/`.

## Invariant 69: User-Intent Preservation & Anti-Feature-Stripping Gate (PAT-2026-1014-002 / PM-2026-1026-002)
- **Rule**: Agents MUST NOT resolve a bug, calibration issue, or test failure by silently disabling or downgrading an enabled user feature (e.g., switching `ADAPTIVE_BED_MESH` to `FULL_BED_MESH`, disabling `transfer_call`, or stripping mobile call forwarding) without explicit prior user consent and prominent disclosure. Full rule: `C:\Users\jmolina\.gemini\config\rules\verify-autonomous-work.md`.

## Invariant 70: Mandatory Terminal Proposal Summary Gate (GLB-023 / PM-2026-1106-001 / IMP-2026-1106-001)
- **MUST**: Every turn requesting user approval for **Live Write** or **Paid Billing** actions MUST end with a structured `### Proposal Summary` block as the last thing printed to the screen (enumerating `#`, `Target System`, `Proposed Action`, `Scope / Affected Entities`, `Command / Mechanism`, and `Mode`), followed only by a concise approval prompt. Routine read-only turns and completed tasks do not require a `### Proposal Summary` block.
- **MUST NOT**: Never bury Live Write or Paid Billing permission requests in prose subsections or print additional content after the terminal Proposal Summary and approval prompt. Full rule: `C:\Users\jmolina\.gemini\config\rules\orchestrator-delegation-and-narration.md`.

## Historical / Retired Global Invariants Index
The following global invariant numbers have been retired, merged, or consolidated into active Invariants 13–70 and global rules in `C:\Users\jmolina\.gemini\config\rules\*.md` during the 2026-08 and 2026-10 governance unifications. Their numbers are retained and never reused:
- **Invariants 1–12**: Consolidated into core workspace bootstrap and session hygiene protocols (now tracked in `wakeup_bootstrap.py`, `/wakeup`, `/goodbye`, and Invariant 13).
- **Invariants 15–23**: Consolidated into autonomous troubleshooting, verification, and artifact isolation rules (`single-target-troubleshooting.md`, `verify-autonomous-work.md`, Invariants 14, 24, 28, 29).
- **Invariants 25–27**: Consolidated into operational tool hygiene and environment validation contracts (Invariants 28–32).
- **Invariant 38**: Merged into Invariant 14 (Three Strikes Deep Research Transition).
- **Invariants 44–49**: Consolidated into physical hardware, staged deployment, and live ground truth verification suites (`no-guessing.md`, Invariants 39–43).
- **Invariants 54–64**: Consolidated into interactive GUI automation, terminal quoting, subagent lifecycle controls, and narration gates (`powershell-syntax-hardening.md`, `orchestrator-delegation-and-narration.md`, Invariants 50–53, 65–70).
