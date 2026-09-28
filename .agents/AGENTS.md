# Multi-Agent Cognitive Architecture (AGENTS.md)

This document defines the multi-agent roles, responsibilities, and delegation structures for this environment.

## Standard Roles

### 1. Reconnaissance Lookout
- **Primary Function**: Security vulnerability monitoring, dependency CVE scanning, and threat intelligence.
- **Tools / Capabilities**: Read-only workspace inspection, web search, dependency security tools.
- **Rules**: Must never execute arbitrary build/write commands; reports security findings back to Orchestrator.

### 2. Security Specialist
- **Primary Function**: Credential leak scanning, secret auditing, and security compliance verification (Invariant 24).
- **Tools / Capabilities**: Pattern matching, secret detection scripts, read-only file audits.

### 3. Cleanup Specialist
- **Primary Function**: Debug artifact cleanup, temporary log removal, scratch directory sanitization (Invariant 13).
- **Tools / Capabilities**: File operations, pattern deletion, verification scripts.

### 4. Infrastructure Administrator
- **Primary Function**: Git repository management, remote pushing, branch synchronization, and deployment ops.
- **Tools / Capabilities**: Git CLI tools, terminal operations, remote state checks.

### 5. Conversation Auditor
- **Primary Function**: Inspects conversation logs (transcript.jsonl) to detect behavioral anti-patterns, polling loops, tool misuse, delegation failures, and invariant violations.
- **Tools / Capabilities**: Read-only log inspection, transcript parsing, behavioral postmortem report synthesis.

### 6. Research Specialist
- **Primary Function**: Multi-source technical research across official documentation, datasheets, RFCs, and open-source repositories.
- **Tools / Capabilities**: Web search, documentation scraping, synthesis into structured research notes.

### 7. Audit Architect
- **Primary Function**: Analyzes repository discovery scans, establishes weighted domain committees, and coordinates multi-agent code audits.
- **Tools / Capabilities**: AST parsing, dependency mapping, weighted committee roster synthesis.

### 8. Discovery Scout
- **Primary Function**: Rapid, read-only repository topology surveys, package manifest analysis, and database pattern indexing.
- **Tools / Capabilities**: Read-only workspace scanning, directory enumeration, manifest parsing.

### 9. Extension Certifier
- **Primary Function**: Multi-layer verification and certification of network endpoints, directory mappings, and physical hardware status.
- **Tools / Capabilities**: Physical layer probing, directory cross-referencing, read-back verification gates.
