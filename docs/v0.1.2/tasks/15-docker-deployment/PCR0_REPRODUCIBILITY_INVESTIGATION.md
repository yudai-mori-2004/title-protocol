# PCR0 再現性調査 (2026-05-25 〜 26)

> **内部記録です。** タスク15 の Dockerfile を「同じソースから 2 回 `--no-cache`
> ビルドして同じ PCR0 が出る」状態に持っていくまでの試行錯誤を残しています。
> 利用者向けの再現性検証手順は
> [docs/v0.1.2/OPERATIONS_JA.md §6.2](../../OPERATIONS_JA.md) (`build.sh --verify`)。
> 本書は対策が *なぜ* これらになったのか、調査の過程を将来の自分のために
> 残すためのもの。

## 結論

| 項目 | 状態 | 検証データ |
|---|---|---|
| 同じ EC2 / 同じ Docker で 2 回 `--no-cache` ビルド → 同じ PCR0 | **達成** | `bab9ec51...` が 2 回連続で出力 (TEE ノード c5.xlarge, 2026-05-25) |
| Rust バイナリ自体が決定的 (`title-tee` の sha256 一致) | **達成** | `e3ec3de5...` (2 ビルド連続) |
| Squash 後の単一レイヤーが一致 | **達成** | `sha256:643b0764...` (2 ビルド連続) |
| ZKP proof が再現性 PCR0 を正しく commit | **達成** | proof の `public_values.measurement` = `bab9ec51...` |
| 並列ビルド・別マシンビルドでの同一性 | **達成見込み** | 下記の前提を満たす限り、別 EC2 でも同じ結果が出るはず (※未実機検証) |

並列ビルドの前提:
- 同じコミット (`Cargo.lock`, `rust-toolchain.toml`, Dockerfile すべて)
- `linux/amd64` 上で実行 (Dockerfile で `--platform=linux/amd64` 明示)
- ベースイメージのダイジェスト pin が DockerHub から取得可能

## 検証時点での具体値 (2026-05-25)

| 値 | hex |
|---|---|
| 再現可能ビルド PCR0 | `bab9ec51dcefb562f3a42bdf046beab224aa05f94efecdf332172f4e6dc92cc505a2e052e6aa608774d86eaba7a8f231` |
| Rust バイナリ `title-tee` sha256 | `e3ec3de5ab27e46662e2c08de2599ef85c166570afbe4d67be4199e778a070d0` |
| Squash 単一レイヤー sha256 | `643b0764c72a05824d85c08098c823fb6c823ffdd9a53bbd2dd5664a67e9bd01` |
| SP1 guest vkey_hash | `0x00d742a0c7af54b880c0bc27eaff7f8f481cd75d9cd7b2516fea02e9ded29754` |

これらの値は記録時点のソース (commit f8f6b35 以降の Dockerfile + Cargo.toml +
rust-toolchain.toml + Cargo.lock) と SP1 toolchain v6.2.0 と DockerHub の base image
ダイジェストに対する結果。ソースか toolchain か base image のどれかが変わると
値も変わる。ビルドが正しく再現したかどうかを判定する基準は
`deploy/aws/build/registration/measurements.json` に保存された運用中 PCR0 と
`bash deploy/aws/scripts/build.sh --verify` の出力一致性。

## 採用した対策 (重要度順)

### 1. `[profile.release]` で決定的コンパイル (`Cargo.toml`)

```toml
[profile.release]
overflow-checks = true
codegen-units = 1   # 並列コード生成による非決定性を排除
lto = "fat"         # ThinLTO の IR ハッシュランダム性を排除
strip = "symbols"   # build-id を含むデバッグ情報を除去
```

`codegen-units` のデフォルト 16 では LLVM が 16 並列でコード生成し、スレッド
スケジューリングでシンボル順序が変わる。これだけで title-tee バイナリの
sha256 が毎回変わっていた。

### 2. `RUSTFLAGS` でホスト固有パスを除去 (`Dockerfile builder stage`)

```dockerfile
ENV RUSTFLAGS="--remap-path-prefix=/build=/src \
               --remap-path-prefix=/usr/local/cargo=/cargo \
               --remap-path-prefix=/usr/local/rustup=/rustup"
ENV CARGO_INCREMENTAL=0
```

- `--remap-path-prefix` は panic メッセージや DWARF debug info に埋め込まれる
  絶対パスを正規化する。`/build` のリマップが抜けていると WORKDIR が変わると
  バイナリが変わる。
- `CARGO_INCREMENTAL=0` は念のため明示 (release では暗黙的に off だが)。

### 3. ベースイメージ・apt パッケージのバージョン pin

```dockerfile
FROM rust:1.93-bookworm@sha256:1d33950f982ca6411f5e0ee4850be46e03f066f1a9efaeb41922a0e59497c9c2 AS builder
FROM debian:bookworm-slim@sha256:b29f74a267526ae6ea104eed6c46133b0ca70ce812525df8cd5817698f0a624a AS runtime

RUN apt-get install -y \
    ca-certificates=20230311+deb12u1 \
    socat=1.7.4.4-2 \
    iproute2=6.1.0-3
```

`debian:bookworm-slim` のような floating tag を使うと Debian 側の minor
update でレイヤーが変わる。SHA256 ダイジェストで固定する。

### 4. ファイル mtime のクランプ (`runtime stage`)

```dockerfile
RUN find / -newermt '@0' ! -path '/proc/*' ! -path '/sys/*' \
      -exec touch -h -d '@0' {} + 2>/dev/null || true
```

`SOURCE_DATE_EPOCH=0` は dpkg などのプログラムが読む環境変数で、Docker 自身が
作るレイヤー tar のエントリ mtime には**影響しない**。apt が展開したファイル、
COPY されたバイナリのいずれもビルド時刻が mtime として記録されるため、最終
RUN で全ファイルを epoch 0 にリセットする。

### 5. **`FROM scratch` でスカッシュ** (whiteout 問題の唯一の解)

```dockerfile
FROM debian:bookworm-slim@sha256:... AS runtime
RUN apt-get install ... && find / ... touch ...
COPY --from=builder ... 
RUN chmod +x ...

FROM scratch
COPY --from=runtime / /
ENV TEE_RUNTIME=nitro
ENV PROXY_ADDR=vsock://3:8000
ENTRYPOINT ["/usr/local/bin/tee-entrypoint.sh"]
```

これが**最も重要**かつ非自明な対策。

#### なぜ必要か

`rm -rf /var/log/apt` のように**下位レイヤーに存在するファイルを削除**すると、
Docker は overlay 仕様に従って `var/log/.wh.apt` という whiteout エントリを
レイヤー tar に書く。この whiteout エントリの mtime は **Docker がレイヤーを
commit する瞬間の時刻**で、RUN コマンド内からは制御できない。

Nitro Enclave の PCR0 は EIF (ramdisk) 全体のハッシュなので、たった一つの
whiteout の mtime が違うだけで PCR0 が変わる。

#### 既知のバグ

- [moby/buildkit#3168](https://github.com/moby/buildkit/issues/3168) で 2023 年に
  「Overlay snapshotter 使用時は whiteout を epoch 0 にする」修正が入った。
- が、[moby/moby#50063](https://github.com/moby/moby/issues/50063) (2026-04 報告、
  本記録時点で**未解決**) によれば、Docker 28.1.1 でも `rewrite-timestamp=true`
  ですら一部の whiteout で機能しない。

#### `FROM scratch` で回避できる理由

`FROM scratch` イメージには下位レイヤーが存在しない。`COPY --from=runtime / /` は
runtime stage の最終ファイルシステム全体を新規ファイルとしてコピーするだけで、
削除操作ではないため whiteout が一切生成されない。結果として単一のフラット
レイヤーになる。

Nitro Enclave では `nitro-cli build-enclave` が結局イメージをフラット化して
ramdisk を作るので、レイヤー共有によるディスク節約のメリットも失わない。

## 試したが効かなかったアプローチ

| アプローチ | 結果 | 理由 |
|---|---|---|
| `ENV SOURCE_DATE_EPOCH=0` | ✗ | RUN 内のプログラム (dpkg) にしか効かない |
| `ARG SOURCE_DATE_EPOCH=0` | ✗ | BuildKit v0.13+ で導入。EC2 上は v0.12.1 |
| `find / -newermt '@0' -exec touch -h -d '@0' {} +` (全 path) | △ | レイヤーが 16MB → 97MB に肥大化、かつ whiteout は直らない |
| `find /etc /var /usr /tmp ...` (限定 path) | △ | mtime は揃うが whiteout は残る |
| `docker build --build-arg SOURCE_DATE_EPOCH=0` | ✗ | BuildKit v0.12 では未対応 |

## 検証方法

```bash
# EC2 上で
cd ~/title-protocol
bash deploy/aws/scripts/build.sh                # ベースライン PCR0 を取得・記録
bash deploy/aws/scripts/build.sh --verify       # 同じ PCR0 が出るか確認
```

`build.sh --verify` は `--no-cache` で再ビルドし、`measurements.json` に記録
された PCR0 と比較する。一致すれば exit 0、不一致なら exit 1。

実機検証ログ (2026-05-25, TEE ノード c5.xlarge):

```
=== REPRODUCIBILITY VERIFIED ===
    PCR0 matches recorded value:
    bab9ec51dcefb562f3a42bdf046beab224aa05f94efecdf332172f4e6dc92cc505a2e052e6aa608774d86eaba7a8f231
```

所要時間 12.5 分 (754 秒)。

## 別マシンでの再現性

別の EC2 / 別の AL2023 マシンでも、以下が同じであれば同じ PCR0 が出る:

1. リポジトリのコミット (Cargo.lock, rust-toolchain.toml, Dockerfile が一致)
2. `linux/amd64` (Dockerfile で `--platform` 明示済み)
3. ベースイメージのダイジェスト pin (DockerHub に存在し続ける限り)

並列実行で複数マシンが同じ PCR0 を吐くことが保証されるので、CI 上の自動
検証も可能。

## ZKP proof との一気通貫検証

再現性 PCR0 で起動した TEE → registration attestation 取得 → SP1 proof 生成 →
proof の `public_values.measurement` が同じ PCR0 を commit している、まで
通しで確認した (2026-05-25):

```
attestation.bin.public_values.bin parse:
  instance_id:     i-00b3f9b3607e019c2-enc019e60207ea96699
  measurement PCR0: bab9ec51dcefb562f3a42bdf046beab224aa05f94efecdf332172f4e6dc92cc505a2e052e6aa608774d86eaba7a8f231
  user_data_hash:  SHA-256(SHA-256(b"title:solana-key" || solana_pubkey)) と一致
  vkey_hash:       0x00d742a0c7af54b880c0bc27eaff7f8f481cd75d9cd7b2516fea02e9ded29754
```

つまり: 「再現性のあるソース」→「決定的な PCR0」→「PCR0 を commit した
Groth16 proof」→「on-chain で PCR0 allowlist と一致検証」のチェーンが
バイト単位で繋がっている。

## デバッグ用コマンド集

```bash
# 2 つのイメージのレイヤー比較
docker inspect IMG1 --format='{{.RootFS.Layers}}'
docker inspect IMG2 --format='{{.RootFS.Layers}}'

# どのレイヤーまで一致するか
docker save IMG1 -o /tmp/img1.tar
docker save IMG2 -o /tmp/img2.tar
tar -xf /tmp/img1.tar -O manifest.json | python3 -m json.tool
tar -xf /tmp/img2.tar -O manifest.json | python3 -m json.tool

# あるレイヤーのファイル一覧 (mtime 込み)
tar -tvf blobs/sha256/<digest> | head -50

# ファイル内容まで比較
mkdir l1 l2
tar -xf blobs/sha256/<digest1> -C l1
tar -xf blobs/sha256/<digest2> -C l2
diff <(cd l1 && find . -type f -exec sha256sum {} + | sort) \
     <(cd l2 && find . -type f -exec sha256sum {} + | sort)

# ELF の決定性だけ確認
docker create --name tmp IMG && docker cp tmp:/usr/local/bin/title-tee /tmp/title-tee-1 && docker rm tmp
# rebuild...
docker create --name tmp IMG && docker cp tmp:/usr/local/bin/title-tee /tmp/title-tee-2 && docker rm tmp
sha256sum /tmp/title-tee-1 /tmp/title-tee-2
```

## 参考

- [reproducible-builds.org — Rust](https://reproducible-builds.org/docs/rust/)
- [AWS — Reproducible builds and AWS Nitro Enclaves](https://aws.amazon.com/blogs/web3/establishing-verifiable-security-reproducible-builds-and-aws-nitro-enclaves/)
- [Trail of Bits — A few notes on AWS Nitro Enclaves images and attestation](https://blog.trailofbits.com/2024/02/16/a-few-notes-on-aws-nitro-enclaves-images-and-attestation/)
- [moby/moby#50063 — Whiteout file timestamps not reproducible](https://github.com/moby/moby/issues/50063)
- [moby/buildkit#3168 — Make whiteout timestamps reproducible](https://github.com/moby/buildkit/issues/3168)
- [Bit-for-bit reproducible builds with Dockerfile (Akihiro Suda)](https://medium.com/nttlabs/bit-for-bit-reproducible-builds-with-dockerfile-7cc2b9faed9f)
