# keys/

Solana keypair files used by the devnet / mock-runtime flow.

| File | 用途 |
|---|---|
| `admin.json` | devnet 上の `programs/title-whitelist` 管理者鍵 (`ADMIN_AUTHORITY`)。OPERATIONS_JA §2.x の `add_approved_vkey` / `add_approved_measurement` / `revoke_key` 等の admin 操作で読み込まれる |

**全ての `*.json` / `*.pem` は `.gitignore` 対象** (`!keys/README.md` だけ allowlist しているため、この README のみ tracked)。秘密鍵を絶対にコミットしないこと。

## 新規セットアップ

```bash
solana-keygen new --outfile keys/admin.json
solana airdrop 2 $(solana-keygen pubkey keys/admin.json) --url devnet
```

`ADMIN_AUTHORITY` 定数 (`programs/title-whitelist/src/lib.rs:50`) は固定 pubkey で hard-code されているため、デフォルトの admin.json と異なる鍵で運用する場合は両方を揃えて program を再 deploy する必要がある。
