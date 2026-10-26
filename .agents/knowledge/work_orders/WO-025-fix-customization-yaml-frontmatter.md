# Work Order: WO-025 — Audit and Fix `deep-code-analysis/SKILL.md` and All Similar Customization YAML Frontmatter Issues

- **Work Order ID:** `WO-025`
- **Timestamp:** `2026-10-21T10:04:00-04:00`
- **Assigned Role:** `Infrastructure Administrator / Cleanup Specialist`
- **Status:** `[IN PROGRESS]`

## 1. Objective & Scope
Fix the Antigravity customization loading error:
`Skill deep-code-analysis/SKILL.md: failed to parse frontmatter: yaml: line 2: mapping values are not allowed in this context`
and systematically scan and fix **all** similar customization/frontmatter issues across:
1. `/home/console-admin/Github/squeezelite-esp32/.agents/` (and `.gemini/` if present)
2. `/home/console-admin/.gemini/config/` (all skills, rules, plugins, agents, hooks, and generator scripts/templates)
3. All other workspaces under `/home/console-admin/Github/*/.agents/` and `/home/console-admin/Github/*/.gemini/`

## 2. Root Cause & Required Remediation
1. **Immediate Root Cause Identified**:
   - `/home/console-admin/Github/squeezelite-esp32/.agents/skills/deep-code-analysis/SKILL.md` has unquoted colons (`: `) inside `description:` on line 3 (YAML line 2):
     `description: Project-specialized deep code analysis engine for squeezelite-esp32 (Profile 1: Embedded IoT & MCU + Profile 6: Polyglot Rust Workspace).`
   - Any unquoted YAML scalar containing `: `, `[`, `]`, `{`, `}`, `#`, `&`, `*`, `!`, `|`, `>`, `'`, `"`, `%`, `@`, or `` ` `` can fail or misparse in Go's `gopkg.in/yaml.v3` parser used by Antigravity.
2. **Comprehensive Scan**:
   - Write a Python audit script `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/audit_customizations.py` that scans every `SKILL.md`, rule `.md`, workflow `.md`, agent `.md`, `plugin.json`, `hooks.json`, `skills.json`, `plugins.json`, and `mcp_config.json` under:
     - `/home/console-admin/Github/`
     - `/home/console-admin/.gemini/`
   - Check for:
     a) YAML frontmatter blocks (`--- ... ---`) that fail `yaml.safe_load` OR have unquoted `description:` / `name:` / `trigger:` values containing `: ` or other YAML syntax hazards.
     b) Missing `---` frontmatter in `SKILL.md` files or missing required `name` / `description` fields.
     c) Invalid JSON in any `plugin.json`, `hooks.json`, `skills.json`, `plugins.json`, or `mcp_config.json`.
     d) Any generator scripts or templates in `/home/console-admin/.gemini/config/` (search for `GENERATOR_VERSION` or `Project-specialized` or `TARGET_PROFILES`) that generate `SKILL.md` files without quoting `description: "..."`.
3. **Rule 35 Backups & Fixes**:
   - Before modifying any file, copy a `.bak` backup into `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/` and record every backup path.
   - Fix all broken/unquoted YAML frontmatter using `edit_file` (never use `sed`/`awk`/`cat`).
   - Also fix any generator script/template so future runs always emit valid quoted YAML frontmatter.
4. **Independent Verification**:
   - Re-run `/home/console-admin/.gemini/antigravity/brain/83b36e23-e01c-4db2-9206-9baaa4347f77/scratch/audit_customizations.py` and confirm **0 errors and 0 unquoted colon hazards** across all scanned files (`exit code 0`).

---

## Fulfillment Section
*(To be completed by the subagent)*
