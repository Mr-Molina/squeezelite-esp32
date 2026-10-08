# Scout Discovered Upstream Dependency & Attack Surface Remediation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remediate all upstream dependency vulnerabilities, attack surface exposures, and toolchain security advisories discovered during the Reconnaissance Lookout / Discovery Scout scan on branch `milestone-mk-5`.

**Architecture:** Apply upstream patch for Nanopb memory leak (CVE-2024-53984) in compiled C code, eradicate dormant CivetWeb dead-code attack surface (CVE-2025-9648, CVE-2026-5789), harden cJSON input length boundaries in `wifi-manager` (CVE-2026-16554), modernize web application manifests and lock down dev-server networking to `127.0.0.1`.

**Tech Stack:** C (Nanopb, cJSON, ESP-IDF `esp_http_server`), CMake, Node.js / Webpack, FreeRTOS / ESP32.

**Spec:** Reconnaissance Lookout Assessment Report (`WO-WAKE-09`) & Multi-Agent Cognitive Governance Architecture.

---

## Global Constraints
- Must maintain 100% build compatibility with ESP-IDF v4.3 and v5.x multi-target environments.
- Must not introduce external cloud dependencies ($0.00 spent, Invariant 46).
- Must preserve all existing codebase invariants and function docstrings (Invariant 36).
- Must verify all command exit codes and test outputs before declaring success (Invariant 37, Invariant 39).
- Must zeroize and protect credentials in RAM and prevent plain-text leakages (Invariant 24).

## Review Focus
1. `pb_decode_ex()` error handling when `pb_close_string_substream` fails: verify `pb_release()` is called and dynamically allocated fields are freed without double-free.
2. Removal of `components/spotify/cspot/bell/external/civetweb`: verify CMake build system generates cleanly without error when `BELL_DISABLE_WEBSERVER ON` is set.
3. cJSON input length bounds: verify incoming HTTP payloads exceeding buffer capacity are rejected before reaching `cJSON_Parse()`.
4. Webpack dev server bind: verify server binds strictly to `127.0.0.1` and rejects unauthorized cross-origin requests.
5. Webapp unit tests: verify all existing `test/test.js` helper tests pass cleanly after package dependency updates.

---

### Task 1: Nanopb Security Patch for Memory Leak / DoS (CVE-2024-53984)

**Files:**
- Modify: `components/spotify/cspot/bell/external/nanopb/pb_decode.c:1155-1175`
- Test: `scratch/test_nanopb_cve_2024_53984.py`

**Interfaces:**
- Consumes: `pb_decode_ex()`, `pb_close_string_substream()`, `pb_release()`
- Produces: Hardened `pb_decode_ex()` ensuring zero memory leaks under `PB_ENABLE_MALLOC` on delimited substream errors.

- [ ] **Step 1: Write the failing verification test**

Create a Python / C verification harness in `scratch/test_nanopb_cve_2024_53984.py` inspecting `pb_decode.c` line logic and asserting that `pb_close_string_substream` failure assigns `status = false` instead of immediate return.

```python
# scratch/test_nanopb_cve_2024_53984.py
import re
import sys

def verify_nanopb_patch(file_path):
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Look for pb_decode_ex implementation
    pattern = r"bool\s+checkreturn\s+pb_decode_ex\s*\([^)]*\)\s*\{([\s\S]*?)\n\}"
    match = re.search(pattern, content)
    assert match, "pb_decode_ex not found"
    body = match.group(1)

    # Check if pb_close_string_substream failure does status = false instead of return false
    bad_pattern = r"if\s*\(!pb_close_string_substream\(stream,\s*&substream\)\)\s*return\s+false;"
    good_pattern = r"if\s*\(!pb_close_string_substream\(stream,\s*&substream\)\)\s*status\s*=\s*false;"

    if re.search(bad_pattern, body):
        print("FAIL: pb_decode_ex contains unpatched early return leading to memory leak (CVE-2024-53984)")
        return False
    elif re.search(good_pattern, body):
        print("PASS: pb_decode_ex correctly assigns status = false to invoke pb_release()")
        return True
    else:
        print("FAIL: unexpected pb_close_string_substream error pattern")
        return False

if __name__ == "__main__":
    target = "components/spotify/cspot/bell/external/nanopb/pb_decode.c"
    success = verify_nanopb_patch(target)
    sys.exit(0 if success else 1)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python scratch/test_nanopb_cve_2024_53984.py`  
Expected output: `FAIL: pb_decode_ex contains unpatched early return leading to memory leak (CVE-2024-53984)` (Exit code 1).

- [ ] **Step 3: Apply the upstream Nanopb 0.4.9.1 patch**

In `components/spotify/cspot/bell/external/nanopb/pb_decode.c` around line 1162, replace:

```c
      if (!pb_close_string_substream(stream, &substream))
        return false;
```

with:

```c
      if (!pb_close_string_substream(stream, &substream))
        status = false;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python scratch/test_nanopb_cve_2024_53984.py`  
Expected output: `PASS: pb_decode_ex correctly assigns status = false to invoke pb_release()` (Exit code 0).

- [ ] **Step 5: Commit**

```bash
git add components/spotify/cspot/bell/external/nanopb/pb_decode.c
git commit -m "fix(nanopb): resolve memory leak on delimited substream close (CVE-2024-53984)"
```

---

### Task 2: Purge Dormant CivetWeb Attack Surface (CVE-2025-9648, CVE-2026-5789)

**Files:**
- Delete: `components/spotify/cspot/bell/external/civetweb/`
- Modify: `components/spotify/cspot/bell/CMakeLists.txt:303-310`

**Interfaces:**
- Consumes: `BELL_DISABLE_WEBSERVER` build option.
- Produces: Cleaned CMake dependency graph with zero CivetWeb scanner vulnerabilities.

- [ ] **Step 1: Verify zero active call sites to CivetWeb in compiled firmware**

Verify that `components/spotify/CMakeLists.txt` sets `set(BELL_DISABLE_WEBSERVER ON)` and that no firmware files include CivetWeb headers.

- [ ] **Step 2: Purge `components/spotify/cspot/bell/external/civetweb/` directory**

Delete the vendored `civetweb` directory.

- [ ] **Step 3: Update `components/spotify/cspot/bell/CMakeLists.txt`**

Replace lines 303-310 in `components/spotify/cspot/bell/CMakeLists.txt`:

```cmake
if(NOT BELL_DISABLE_WEBSERVER)
    message(FATAL_ERROR "Built-in CivetWeb server has been purged due to CVE-2025-9648. Squeezelite-esp32 uses native esp_http_server.")
else()
    list(REMOVE_ITEM SOURCES "${IO_DIR}/BellHTTPServer.cpp")    
    list(REMOVE_ITEM SOURCES "${IO_DIR}/MGStreamAdapter.cpp")    
endif()
```

- [ ] **Step 4: Verify CMake parsing syntax**

Run: `cmake -P -` with a test script verifying that `components/spotify/cspot/bell/CMakeLists.txt` syntax is valid.

- [ ] **Step 5: Commit**

```bash
git add components/spotify/cspot/bell/CMakeLists.txt
git rm -rf components/spotify/cspot/bell/external/civetweb
git commit -m "fix(security): purge dormant civetweb attack surface (CVE-2025-9648, CVE-2026-5789)"
```

---

### Task 3: cJSON Defense-in-Depth Payload Bounds & Deserialization Guards (CVE-2026-16554)

**Files:**
- Modify: `components/wifi-manager/http_server_handlers.c`
- Test: `scratch/test_cjson_guards.py`

**Interfaces:**
- Consumes: `post_handler_buff_receive()`, `cJSON_Parse()`, `cJSON_GetObjectItemCaseSensitive()`
- Produces: Bounded string deserialization rejecting inputs exceeding protocol limits.

- [ ] **Step 1: Write the failing verification test**

Create `scratch/test_cjson_guards.py` validating that string lengths extracted from parsed JSON in `http_server_handlers.c` are guarded by explicit bounds (`MAX_SSID_LEN`, `MAX_PASSWORD_LEN`, `MAX_COMMAND_LEN`).

```python
# scratch/test_cjson_guards.py
import re
import sys

def verify_cjson_length_checks(file_path):
    with open(file_path, "r", encoding="utf-8") as f:
        content = f.read()

    # Verify wifi_connect_post_handler validates SSID and password length
    has_ssid_len_guard = "strlen(ssid_object->valuestring) <= 32" in content or "strlen(ssid_object->valuestring) < 33" in content or "sizeof(network_config.ssid)" in content
    has_pwd_len_guard = "strlen(pwd_object->valuestring) <= 64" in content or "strlen(pwd_object->valuestring) < 65" in content or "sizeof(network_config.password)" in content

    if not (has_ssid_len_guard and has_pwd_len_guard):
        print("FAIL: Missing explicit length bound check on deserialized cJSON strings")
        return False
    print("PASS: cJSON string deserialization includes explicit boundary checks")
    return True

if __name__ == "__main__":
    success = verify_cjson_length_checks("components/wifi-manager/http_server_handlers.c")
    sys.exit(0 if success else 1)
```

- [ ] **Step 2: Run test to verify it fails**

Run: `python scratch/test_cjson_guards.py`  
Expected output: `FAIL: Missing explicit length bound check on deserialized cJSON strings` (Exit code 1).

- [ ] **Step 3: Implement boundary checks in `components/wifi-manager/http_server_handlers.c`**

Add explicit bounds checking to `wifi_connect_post_handler` ensuring `ssid_object->valuestring` length is $\le 32$ and `pwd_object->valuestring` length is $\le 64$ before copying into `network_config`:

```c
	if(cJSON_IsString(ssid_object) && ssid_object->valuestring != NULL){
		if (strlen(ssid_object->valuestring) >= sizeof(network_config.ssid)) {
			ESP_LOGE_LOC(TAG, "SSID exceeds maximum buffer length");
			httpd_resp_send_err(req, HTTPD_400_BAD_REQUEST, "SSID too long");
			cJSON_Delete(root);
			return ESP_FAIL;
		}
		strncpy((char *)network_config.ssid, ssid_object->valuestring, sizeof(network_config.ssid) - 1);
		network_config.ssid[sizeof(network_config.ssid) - 1] = '\0';
	}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `python scratch/test_cjson_guards.py`  
Expected output: `PASS: cJSON string deserialization includes explicit boundary checks` (Exit code 0).

- [ ] **Step 5: Commit**

```bash
git add components/wifi-manager/http_server_handlers.c
git commit -m "fix(wifi-manager): add strict length validation to cJSON string extraction (CVE-2026-16554)"
```

---

### Task 4: Web Application Manifest Hardening & Dev Server Security

**Files:**
- Modify: `components/wifi-manager/webapp/package.json`
- Modify: `components/wifi-manager/webapp/webpack/webpack.dev.js`
- Test: `components/wifi-manager/webapp/test/test.js`

**Interfaces:**
- Consumes: npm dependency manifests and webpack dev-server network bindings.
- Produces: Hardened package manifest with zero runtime-misclassified dependencies and localhost-restricted dev server.

- [ ] **Step 1: Update `package.json`**

1. Move `"optipng-bin": "^9.0.0"` from `dependencies` to `devDependencies`.
2. Update `"lodash": "^4.18.1"`, `"lodash-es": "^4.18.1"`.
3. Update `"postcss": "^8.5.23"`.
4. Update `"webpack-dev-server": "^5.2.6"`.

- [ ] **Step 2: Restrict dev-server network binding in `webpack/webpack.dev.js`**

In `components/wifi-manager/webapp/webpack/webpack.dev.js`:
1. Change `host: '0.0.0.0'` to `host: '127.0.0.1'`.
2. Change `allowedHosts: "all"` to `allowedHosts: ["localhost", "127.0.0.1"]`.
3. Change `'Access-Control-Allow-Origin': '*'` to `'Access-Control-Allow-Origin': 'http://127.0.0.1:5000'`.

- [ ] **Step 3: Run webapp unit test suite**

Run: `node components/wifi-manager/webapp/test/test.js`  
Expected output: All 6 webapp unit tests passed successfully (Exit code 0).

- [ ] **Step 4: Commit**

```bash
git add components/wifi-manager/webapp/package.json components/wifi-manager/webapp/webpack/webpack.dev.js
git commit -m "fix(webapp): modernize dependencies and bind dev-server strictly to localhost"
```

---

### Task 5: Security Re-Scan & Multi-Agent Ledger Certification

**Files:**
- Modify: `.agents/sessions.md`

**Interfaces:**
- Consumes: Reconnaissance Lookout re-scan, Invariant 24 secret check, Invariant 13 cleanup check.
- Produces: Certified Clean session ledger entry in `.agents/sessions.md`.

- [ ] **Step 1: Dispatch Reconnaissance Lookout to re-verify dependencies**

Dispatch `recon-lookout` to re-audit the workspace. Confirm that CivetWeb is absent, Nanopb is patched, and manifests are updated with security floors.

- [ ] **Step 2: Execute Invariant 24 Secret Scan and Invariant 13 Debug Cleanup**

Verify 0 secrets committed and remove temporary test scripts from `scratch/`.

- [ ] **Step 3: Update `.agents/sessions.md`**

Prepend new session entry documenting complete remediation of scout findings.

- [ ] **Step 4: Commit**

```bash
git add .agents/sessions.md
git commit -m "docs(governance): record scout discovered vulnerability remediation in session ledger"
```
