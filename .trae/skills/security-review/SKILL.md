---
name: "security-review"
description: "Reviews code for security violations — path traversal, API key leaks, mode switch bypass, data sanitization. Invoke after writing code that touches file paths, API keys, or inference mode switching."
---

# Security Review

Reviews code against the project's security red lines defined in `rules/security.md`.

## When to Invoke

- After writing code that handles file paths
- After writing code that touches API keys or credentials
- After writing code related to inference mode switching
- After writing code for cloud API requests
- User asks for "security review" or "security check"
- Before merging security-sensitive modules

## Steps

### 1. Read Security Rules

Read `rules/security.md` to understand the security red lines.

### 2. Path Safety Review

Check ALL file path operations in the modified code:

**MUST verify:**
- [ ] All path parameters pass through `path_guard::validate()` or `path_guard::validate_within_root()`
- [ ] No direct `std::fs::read_dir(&user_path)` without validation
- [ ] No path concatenation without canonicalization (e.g., `format!("{base}/{user_input}")`)
- [ ] Blocked paths (`/System`, `/proc`, `C:\Windows\System32`, etc.) are rejected

**Scan command:**
```bash
# Find path parameters in IPC commands
grep -rn "path: String" src-tauri/src/commands/ --include="*.rs"
# For each result, verify path_guard::validate is called in the same function
```

### 3. API Key Safety Review

Check all code touching credentials:

**MUST verify:**
- [ ] API keys are stored in OS Keychain via `keychain::store_key()`, not hardcoded
- [ ] API keys are read via `keychain::get_key()`, not from env vars or config files
- [ ] Cloud API requests go through Rust proxy (not directly from Python Sidecar)
- [ ] No API key in logs (check `logger.info` / `println!` calls)
- [ ] No API key in error messages returned to frontend

**Scan command:**
```bash
# Check for hardcoded keys
grep -rn "sk-\|api_key\|API_KEY\|secret\|token" src-tauri/src/ python-sidecar/app/ --include="*.rs" --include="*.py" | grep -v "keychain\|test\|comment\|placeholder"

# Check for key in Python (should not exist)
grep -rn "api_key\|API_KEY" python-sidecar/app/ --include="*.py" | grep -v "test"
```

### 4. Mode Switch Safety Review

Check inference mode switching code:

**MUST verify:**
- [ ] Mode switch calls `security::mode_switch::validate_mode_switch()`
- [ ] `source` parameter is checked (auto vs manual)
- [ ] Cloud mode requires user consent (`has_consent()`)
- [ ] Same-mode switch is rejected

**Scan command:**
```bash
# Find mode switch code
grep -rn "set_inference_mode\|switch_mode\|cloud" src-tauri/src/ --include="*.rs" | grep -v "test\|mod.rs"
# Verify validate_mode_switch is called
```

### 5. Cloud Data Sanitization Review

Check code that sends data to cloud APIs:

**MUST verify:**
- [ ] File names are anonymized (e.g., `file_001`, not real names)
- [ ] File paths are masked (e.g., `***`)
- [ ] Content is truncated (max 2000 characters)
- [ ] No PII (personally identifiable information) in cloud requests

### 6. Log Sanitization Review

Check all logging statements:

**MUST verify:**
- [ ] No real file paths in logs (use `sanitize_path()`)
- [ ] No API keys in logs
- [ ] No user query content in logs
- [ ] No file content in logs

**Scan command:**
```bash
# Check for potential log leakage
grep -rn "log\|logger\|info\|debug\|warn\|error" python-sidecar/app/ --include="*.py" | grep -v "import\|test"
```

### 7. Operation Log Chain Review

Check file operation logging:

**MUST verify:**
- [ ] Operations are logged to `operations_log` table
- [ ] Each log entry has `prev_hash` and `current_hash` (SHA-256 chain)
- [ ] `current_hash = SHA256(prev_hash + operation_data)`
- [ ] Chain integrity is verified on startup

### 8. Report

```
=== Security Review Report ===

Path Safety:
  [PASS/FAIL] All paths validated
  [PASS/FAIL] No path traversal vectors
  Details: ...

API Key Safety:
  [PASS/FAIL] Keys in Keychain
  [PASS/FAIL] No hardcoded keys
  [PASS/FAIL] Cloud requests via Rust proxy
  Details: ...

Mode Switch Safety:
  [PASS/FAIL] Mode switch validated
  [PASS/FAIL] Consent check present
  Details: ...

Data Sanitization:
  [PASS/FAIL] File names anonymized
  [PASS/FAIL] Paths masked
  [PASS/FAIL] Content truncated
  Details: ...

Log Sanitization:
  [PASS/FAIL] No sensitive data in logs
  Details: ...

Operation Log:
  [PASS/FAIL] Chain hash present
  [PASS/FAIL] Startup verification
  Details: ...

=== Result: ALL CHECKS PASSED / N ISSUES FOUND ===
```

If any FAIL: describe the violation, the file/line, and the fix needed. Do NOT proceed with merge until all security issues are resolved.
