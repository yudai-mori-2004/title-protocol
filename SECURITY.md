# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Title Protocol, please report it responsibly.

**Preferred method:** [GitHub Security Advisories](https://github.com/title-protocol/title-protocol/security/advisories/new)

**Alternative:** Email `contact@titleprotocol.org`

**Please do NOT:**
- Open a public GitHub issue for security vulnerabilities
- Disclose the vulnerability publicly before it has been addressed

## Scope

The following components are in scope for security reports:

| Component | Path | Examples |
|-----------|------|---------|
| TEE Server | `crates/tee/` | Authentication bypass, key extraction, attestation spoofing |
| Gateway | `crates/gateway/` | Request forgery, authorization bypass, SSRF |
| Proxy | `crates/proxy/` | Protocol injection, data interception |
| Cryptography | `crates/crypto/` | Key derivation flaws, nonce reuse, weak randomness |
| TypeScript SDK | `sdk/ts/` | Encryption/decryption bugs, key handling errors |
| Solana Program | `programs/title-config/` | Privilege escalation, unauthorized config changes |
| WASM Modules | `wasm/` | Memory safety, sandbox escape |

### Out of Scope

- `experiments/` — Development experiments, not deployed
- `docs/` — Documentation only

## Response Timeline

| Stage | Target |
|-------|--------|
| Acknowledgment | Within 48 hours |
| Triage & severity assessment | Within 7 days |
| Fix for Critical/High | Best effort, typically within 30 days |
| Fix for Medium/Low | Next planned release |

## Severity Classification

We follow a standard severity scale:

- **Critical:** Remote code execution, key extraction from TEE, full bypass of authentication
- **High:** Unauthorized minting of cNFTs, GlobalConfig manipulation, data exfiltration
- **Medium:** Denial of service, information disclosure of non-sensitive data
- **Low:** Minor issues with limited impact

## Recognition

We appreciate responsible disclosure and will acknowledge security researchers in release notes (with permission).
