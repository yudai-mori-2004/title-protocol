// SPDX-License-Identifier: Apache-2.0

//! # title-proxy library surface
//!
//! Spec §5.2 — HTTP forwarder between Nitro Enclave and the public network.
//!
//! 本 crate は binary (`src/main.rs`) と library (本ファイル) を両方提供する。
//! library 部は **wire protocol 定義と handler ロジックを TEE 側 (title-tee
//! crate) からも参照できるようにする**ためのもの。
//!
//! Enclave 側 (title-tee::proxy_fetcher) と host 側 (title-proxy::main) は
//! 同じワイヤープロトコルで通信するため、定数 / エンコーディングを 1 箇所
//! (`protocol`) で管理する。`handler` は host 側でしか使わないが、テスト時に
//! tee crate から in-process で叩く場面があるかもしれないので合わせて公開する。

pub mod handler;
pub mod protocol;
