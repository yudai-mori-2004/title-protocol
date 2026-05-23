# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Title Protocol, please report it responsibly.

**Preferred method:** [GitHub Security Advisories](https://github.com/yudai-mori-2004/title-protocol/security/advisories/new)

**Alternative:** Email `contact@titleprotocol.org`

**Please do NOT:**
- Open a public GitHub issue for security vulnerabilities
- Disclose the vulnerability publicly before it has been addressed

## Scope

The following components are in scope for security reports:

| Component | Description | Examples |
|-----------|-------------|---------|
| TEE Server | Trusted Execution Environment server | Key extraction, attestation spoofing, memory isolation bypass |
| Gateway | HTTP API server | Request forgery, authorization bypass, SSRF |
| Processors | Attribute extraction modules | Memory safety, input validation, hash collision exploitation |
| Cryptography | Encryption suites and key management | Key derivation flaws, nonce reuse, weak randomness |
| Solana Extension | On-chain integration | ZK proof bypass, whitelist manipulation, unauthorized minting |

### Out of Scope

- `legacy/` -- Archived code, not deployed
- `docs/` -- Documentation only

## Response Timeline

| Stage | Target |
|-------|--------|
| Acknowledgment | Within 48 hours |
| Triage & severity assessment | Within 7 days |
| Fix for Critical/High | Best effort, typically within 30 days |
| Fix for Medium/Low | Next planned release |

## Severity Classification

- **Critical:** Key extraction from TEE, attestation forgery, full authentication bypass
- **High:** Unauthorized cNFT minting, whitelist manipulation, content data exfiltration
- **Medium:** Denial of service (OOM, resource exhaustion), information disclosure of non-sensitive data
- **Low:** Minor issues with limited impact

## Recognition

We appreciate responsible disclosure and will acknowledge security researchers in release notes (with permission).
