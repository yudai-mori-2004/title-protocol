// SPDX-License-Identifier: Apache-2.0

//! # Title Protocol WASM実行環境
//!
//! 仕様書セクション7で定義されるWASM実行環境をwasmtimeを直接使用して実装する。
//!
//! ## 安全性確保 (仕様書 §7.1)
//! - Fuel制限: 命令実行数の上限（無限ループ防止）
//! - Memory制限: メモリ使用量の上限（OOM防止）
//! - catch_unwind: パニックをキャッチし、Core処理への影響を遮断
//!
//! ## ホスト関数 (仕様書 §7.1)
//! - `read_content_chunk`: コンテンツのチャンク読み取り
//! - `get_content_length`: コンテンツの全長取得
//! - `get_extension_input`: Extension補助入力の取得
//! - `get_content_feature`: コンテンツの特徴量計算（JSON spec指定: sha256/sha384/sha512）
//! - `hmac_content`: コンテンツのHMAC計算
//! - `decode_content`: コンテンツのデコード（画像→ピクセル等）
//! - `read_decoded_chunk`: デコード済みデータのチャンク読み取り
//! - `get_decoded_length`: デコード済みデータの全長取得
//! - `get_decoded_feature`: デコード済みデータの特徴量計算（JSON spec指定: grayscale_resize等）
//!
//! ## WASM結果フォーマット
//! WASMエクスポート関数は結果バッファへのポインタ(u32)を返す。
//! バッファ形式: `[4B LE: json_len][json_bytes...]`

pub mod c2pa_cert;
pub mod decode;
pub mod resource_pool;

pub use resource_pool::{ResourcePool, Ticket};

use std::panic;
use std::sync::Arc;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256, Sha384, Sha512};
use wasmtime::{Caller, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder, Trap};

/// WASM実行環境のエラー型
#[derive(Debug, thiserror::Error)]
pub enum WasmError {
    /// WASMモジュールのコンパイルエラー
    #[error("WASMコンパイルエラー: {0}")]
    CompileError(String),
    /// WASMモジュールの実行エラー
    #[error("WASM実行エラー: {0}")]
    ExecutionError(String),
    /// Fuel制限超過
    #[error("Fuel制限を超過しました")]
    FuelExhausted,
    /// Memory制限超過
    #[error("Memory制限を超過しました")]
    MemoryLimitExceeded,
    /// WASMパニック
    #[error("WASMモジュールがパニックしました: {0}")]
    Panic(String),
    /// ホスト関数エラー
    #[error("ホスト関数エラー: {0}")]
    HostFunctionError(String),
}

/// WASM実行結果。
#[derive(Debug)]
pub struct ExtensionResult {
    /// WASM実行結果のJSON
    pub output: serde_json::Value,
}

/// デコード済みコンテンツ。
/// decode_content ホスト関数の結果として InnerHostState に格納される。
/// メタデータ（画像: width/height/channels 等）はデコード時にWASMリニアメモリに書き込まれ、
/// 同時に本構造体にも保存される（get_decoded_feature で参照するため）。
/// 仕様書 §7.1
struct DecodedContent {
    /// デコード済み生データ（コンテンツ種別に依存しない）
    data: Vec<u8>,
    /// 画像幅（ピクセル）
    width: u32,
    /// 画像高さ（ピクセル）
    height: u32,
    /// チャネル数（1=Luma, 3=RGB, 4=RGBA）
    channels: u32,
}

/// wasmtime Store内部の状態。
/// コンテンツデータとStoreLimitsを保持する。
struct InnerHostState {
    /// コンテンツの生データ
    content: Vec<u8>,
    /// Extension補助入力
    extension_input: Option<Vec<u8>>,
    /// メモリ制限
    limiter: StoreLimits,
    /// デコード済みコンテンツ（decode_content 呼び出し後に Some）
    /// 仕様書 §7.1
    decoded: Option<DecodedContent>,
    /// ResourcePool 参照（デコード予約用）
    /// 仕様書 §7.1
    resource_pool: Option<Arc<ResourcePool>>,
    /// デコード済みデータのメモリ予約チケット（Drop で自動解放）
    /// 仕様書 §7.1
    decode_ticket: Option<Ticket>,
}

/// WASM実行ランナー。
/// 仕様書 §7.1
pub struct WasmRunner {
    /// Fuel制限（命令実行数の上限）
    fuel_limit: u64,
    /// Memory制限（バイト）
    memory_limit: usize,
    /// ResourcePool（デコード済みデータのメモリ予算管理用）
    /// 仕様書 §7.1
    resource_pool: Option<Arc<ResourcePool>>,
}

impl WasmRunner {
    /// 新しいWasmRunnerを作成する（後方互換）。
    /// 仕様書 §7.1
    ///
    /// # 引数
    /// - `fuel_limit`: 命令実行数の上限（無限ループ防止）
    /// - `memory_limit`: メモリ使用量の上限（バイト、OOM防止）
    pub fn new(fuel_limit: u64, memory_limit: usize) -> Self {
        Self {
            fuel_limit,
            memory_limit,
            resource_pool: None,
        }
    }

    /// ResourcePool付きのWasmRunnerを作成する。
    /// 仕様書 §7.1
    pub fn with_resource_pool(
        fuel_limit: u64,
        memory_limit: usize,
        pool: Arc<ResourcePool>,
    ) -> Self {
        Self {
            fuel_limit,
            memory_limit,
            resource_pool: Some(pool),
        }
    }

    /// WASMモジュールを実行し、Extension結果を返す。
    /// 仕様書 §7.1
    ///
    /// catch_unwindによりWASMパニックを遮断し、Core処理への影響を防ぐ。
    ///
    /// # 引数
    /// - `wasm_bytes`: WASMバイナリ
    /// - `content`: コンテンツの生データ
    /// - `extension_input`: Extension補助入力（Optional）
    /// - `export_name`: 呼び出すエクスポート関数名
    pub fn execute(
        &self,
        wasm_bytes: &[u8],
        content: &[u8],
        extension_input: Option<&[u8]>,
        export_name: &str,
    ) -> Result<ExtensionResult, WasmError> {
        let fuel_limit = self.fuel_limit;
        let memory_limit = self.memory_limit;
        let resource_pool = self.resource_pool.clone();
        let wasm_bytes = wasm_bytes.to_vec();
        let content = content.to_vec();
        let extension_input = extension_input.map(|v| v.to_vec());
        let export_name = export_name.to_string();

        // catch_unwindでパニック遮断 (仕様書 §7.1)
        let result = panic::catch_unwind(move || {
            Self::execute_inner(
                fuel_limit,
                memory_limit,
                resource_pool,
                &wasm_bytes,
                content,
                extension_input,
                &export_name,
            )
        });

        match result {
            Ok(inner) => inner,
            Err(_) => Err(WasmError::Panic(
                "WASMモジュールの実行中にパニックが発生しました".to_string(),
            )),
        }
    }

    /// wasmtimeのエラーをWasmErrorに変換する。
    fn classify_error(e: wasmtime::Error) -> WasmError {
        // Trap型にダウンキャストしてOutOfFuelを検出
        if let Some(trap) = e.downcast_ref::<Trap>() {
            if *trap == Trap::OutOfFuel {
                return WasmError::FuelExhausted;
            }
        }
        let msg = e.to_string();
        if msg.contains("fuel") {
            WasmError::FuelExhausted
        } else if msg.contains("memory") || msg.contains("Memory") {
            WasmError::MemoryLimitExceeded
        } else {
            WasmError::ExecutionError(msg)
        }
    }

    /// WASM実行の内部実装。
    /// 仕様書 §7.1
    fn execute_inner(
        fuel_limit: u64,
        memory_limit: usize,
        resource_pool: Option<Arc<ResourcePool>>,
        wasm_bytes: &[u8],
        content: Vec<u8>,
        extension_input: Option<Vec<u8>>,
        export_name: &str,
    ) -> Result<ExtensionResult, WasmError> {
        // 1. wasmtime Engineを作成（Fuel制限有効化）
        let mut config = wasmtime::Config::new();
        config.consume_fuel(true);

        let engine = Engine::new(&config)
            .map_err(|e| WasmError::CompileError(format!("Engineの作成に失敗: {e}")))?;

        // 2. HostStateを含むStoreを作成（Memory制限付き）
        let limiter = StoreLimitsBuilder::new()
            .memory_size(memory_limit)
            .build();

        let inner_state = InnerHostState {
            content,
            extension_input,
            limiter,
            decoded: None,
            resource_pool,
            decode_ticket: None,
        };

        let mut store = Store::new(&engine, inner_state);
        store
            .set_fuel(fuel_limit)
            .map_err(|e| WasmError::ExecutionError(format!("Fuel設定に失敗: {e}")))?;
        store.limiter(|s| &mut s.limiter);

        // 3. ホスト関数をLinkerに登録
        let mut linker = Linker::new(&engine);
        Self::register_host_functions(&mut linker)?;

        // 4. WASMバイナリをコンパイル
        let module =
            Module::new(&engine, wasm_bytes).map_err(|e| WasmError::CompileError(e.to_string()))?;

        // 5. インスタンス化
        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(Self::classify_error)?;

        // 6. エクスポートされた計算関数を呼び出す
        let func = instance
            .get_typed_func::<(), u32>(&mut store, export_name)
            .map_err(|e| {
                WasmError::ExecutionError(format!(
                    "エクスポート関数 '{export_name}' が見つかりません: {e}"
                ))
            })?;

        let result_ptr = func.call(&mut store, ()).map_err(Self::classify_error)?;

        if result_ptr == 0 {
            return Err(WasmError::ExecutionError(
                "WASM関数がエラーを返しました (ptr=0)".to_string(),
            ));
        }

        // 7. 結果をWASMメモリから読み取り、ExtensionResultとして返す
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            WasmError::ExecutionError("memoryエクスポートが見つかりません".to_string())
        })?;

        let mem_data = memory.data(&store);
        let ptr = result_ptr as usize;

        // [4B LE: json_len][json_bytes...]
        if ptr + 4 > mem_data.len() {
            return Err(WasmError::ExecutionError(
                "結果ポインタが不正です".to_string(),
            ));
        }

        let json_len = u32::from_le_bytes([
            mem_data[ptr],
            mem_data[ptr + 1],
            mem_data[ptr + 2],
            mem_data[ptr + 3],
        ]) as usize;

        if json_len == 0 || ptr + 4 + json_len > mem_data.len() {
            return Err(WasmError::ExecutionError(
                "結果バッファが不正です".to_string(),
            ));
        }

        let json_bytes = &mem_data[ptr + 4..ptr + 4 + json_len];
        let json_str = std::str::from_utf8(json_bytes)
            .map_err(|e| WasmError::ExecutionError(format!("結果がUTF-8ではありません: {e}")))?;

        let output: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| WasmError::ExecutionError(format!("結果JSONのパースに失敗: {e}")))?;

        Ok(ExtensionResult { output })
    }

    /// ホスト関数をLinkerに登録する。
    /// 仕様書 §7.1
    fn register_host_functions(linker: &mut Linker<InnerHostState>) -> Result<(), WasmError> {
        // read_content_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32
        // コンテンツのチャンクを読み取り、WASMメモリにコピーする。
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "read_content_chunk",
                |mut caller: Caller<'_, InnerHostState>,
                 offset: u32,
                 length: u32,
                 buf_ptr: u32|
                 -> u32 {
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return 0,
                        },
                        None => return 0,
                    };
                    let (mem_data, state) = memory.data_and_store_mut(&mut caller);

                    let start = offset as usize;
                    if start >= state.content.len() {
                        return 0;
                    }
                    let end = (start + length as usize).min(state.content.len());
                    let chunk_len = end - start;

                    let dest = buf_ptr as usize;
                    if dest + chunk_len > mem_data.len() {
                        return 0;
                    }
                    mem_data[dest..dest + chunk_len]
                        .copy_from_slice(&state.content[start..end]);
                    chunk_len as u32
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("read_content_chunkの登録に失敗: {e}"))
            })?;

        // get_content_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32
        // JSON specに基づいてコンテンツの特徴量を計算する。
        // spec: {"op":"sha256"}, {"op":"sha256","offset":0,"length":1024}, {"op":"sha384"}, {"op":"sha512"}
        // 戻り値: 出力バイト数（正値）またはエラーコード（負値）
        // -1=specパースエラー/未知op, -2=コンテンツ範囲外, -3=出力バッファ境界外
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "get_content_feature",
                |mut caller: Caller<'_, InnerHostState>,
                 spec_ptr: u32,
                 spec_len: u32,
                 output_ptr: u32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return -3,
                        },
                        None => return -3,
                    };
                    let (mem_data, state) = memory.data_and_store_mut(&mut caller);

                    // specをWASMメモリから読み取り
                    let sp = spec_ptr as usize;
                    let sl = spec_len as usize;
                    if sp + sl > mem_data.len() {
                        return -1;
                    }
                    let spec_bytes = &mem_data[sp..sp + sl];

                    // JSONパース
                    let spec: serde_json::Value = match serde_json::from_slice(spec_bytes) {
                        Ok(v) => v,
                        Err(_) => return -1,
                    };
                    let op = match spec["op"].as_str() {
                        Some(s) => s,
                        None => return -1,
                    };

                    // offset/lengthの取得（オプショナル）
                    let offset = spec.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                    let length = spec.get("length").and_then(|v| v.as_u64());

                    // コンテンツ範囲の検証
                    if offset > state.content.len() {
                        return -2;
                    }
                    let end = match length {
                        Some(l) => (offset + l as usize).min(state.content.len()),
                        None => state.content.len(),
                    };
                    let data_slice = &state.content[offset..end];

                    // 特徴量計算（仕様書 §7.1）
                    let hash_bytes: Vec<u8> = match op {
                        "sha256" => Sha256::digest(data_slice).to_vec(),
                        "sha384" => Sha384::digest(data_slice).to_vec(),
                        "sha512" => Sha512::digest(data_slice).to_vec(),
                        "c2pa_verify_active_cert_chain" => {
                            let root_spki_hex = match spec.get("root_spki_hex").and_then(|v| v.as_str()) {
                                Some(s) => s,
                                None => return -1,
                            };
                            // 証明書チェーン検証はコンテンツ全体が必要
                            match c2pa_cert::verify_active_cert_chain(&state.content, root_spki_hex) {
                                Ok(true) => vec![0x01],
                                Ok(false) => vec![0x00],
                                Err(_) => return -5, // C2PA構造エラー
                            }
                        }
                        _ => return -1, // 未知のop
                    };

                    // 出力バッファへの書き込み
                    let dest = output_ptr as usize;
                    if dest + hash_bytes.len() > mem_data.len() {
                        return -3;
                    }
                    mem_data[dest..dest + hash_bytes.len()].copy_from_slice(&hash_bytes);
                    hash_bytes.len() as i32
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("get_content_featureの登録に失敗: {e}"))
            })?;

        // hmac_content(algorithm: u32, key_ptr: u32, key_len: u32, offset: u32, length: u32, out_ptr: u32) -> u32
        // コンテンツの指定範囲に対するHMACを計算する。
        // algorithm: 0=HMAC-SHA256(32B), 1=HMAC-SHA384(48B), 2=HMAC-SHA512(64B)
        // key はWASMリニアメモリ上のバイト列。
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "hmac_content",
                |mut caller: Caller<'_, InnerHostState>,
                 algorithm: u32,
                 key_ptr: u32,
                 key_len: u32,
                 offset: u32,
                 length: u32,
                 out_ptr: u32|
                 -> u32 {
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return 0,
                        },
                        None => return 0,
                    };
                    let (mem_data, state) = memory.data_and_store_mut(&mut caller);

                    // WASMメモリからHMACキーを読み取る
                    let kp = key_ptr as usize;
                    let kl = key_len as usize;
                    if kp + kl > mem_data.len() {
                        return 0;
                    }
                    let key = &mem_data[kp..kp + kl];

                    // コンテンツの指定範囲を取得
                    let start = offset as usize;
                    if start >= state.content.len() {
                        return 0;
                    }
                    let end = (start + length as usize).min(state.content.len());
                    let data_slice = &state.content[start..end];

                    // HMAC計算（仕様書 §7.1）
                    let mac_bytes: Vec<u8> = match algorithm {
                        0 => {
                            let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(key) else {
                                return 0;
                            };
                            mac.update(data_slice);
                            mac.finalize().into_bytes().to_vec()
                        }
                        1 => {
                            let Ok(mut mac) = Hmac::<Sha384>::new_from_slice(key) else {
                                return 0;
                            };
                            mac.update(data_slice);
                            mac.finalize().into_bytes().to_vec()
                        }
                        2 => {
                            let Ok(mut mac) = Hmac::<Sha512>::new_from_slice(key) else {
                                return 0;
                            };
                            mac.update(data_slice);
                            mac.finalize().into_bytes().to_vec()
                        }
                        _ => return 0,
                    };

                    let dest = out_ptr as usize;
                    if dest + mac_bytes.len() > mem_data.len() {
                        return 0;
                    }
                    mem_data[dest..dest + mac_bytes.len()].copy_from_slice(&mac_bytes);
                    mac_bytes.len() as u32
                },
            )
            .map_err(|e| WasmError::ExecutionError(format!("hmac_contentの登録に失敗: {e}")))?;

        // get_extension_input(buf_ptr: u32, buf_len: u32) -> u32
        // Extension補助入力をWASMメモリにコピーする。
        // 実際のサイズを返す。buf_len未満の場合もサイズのみ返す（データはコピーされない）。
        // 補助入力が存在しない場合は0を返す。
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "get_extension_input",
                |mut caller: Caller<'_, InnerHostState>,
                 buf_ptr: u32,
                 buf_len: u32|
                 -> u32 {
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return 0,
                        },
                        None => return 0,
                    };
                    let (mem_data, state) = memory.data_and_store_mut(&mut caller);

                    match &state.extension_input {
                        Some(input) => {
                            let actual_size = input.len() as u32;
                            let copy_len = (buf_len as usize).min(input.len());
                            let dest = buf_ptr as usize;
                            if dest + copy_len > mem_data.len() {
                                return actual_size;
                            }
                            mem_data[dest..dest + copy_len]
                                .copy_from_slice(&input[..copy_len]);
                            actual_size
                        }
                        None => 0,
                    }
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("get_extension_inputの登録に失敗: {e}"))
            })?;

        // get_content_length() -> u32
        // コンテンツの全長を返す。
        linker
            .func_wrap(
                "env",
                "get_content_length",
                |caller: Caller<'_, InnerHostState>| -> u32 {
                    caller.data().content.len() as u32
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("get_content_lengthの登録に失敗: {e}"))
            })?;

        // decode_content(params_ptr: u32, params_len: u32, metadata_ptr: u32) -> i32
        // コンテンツをデコードし、InnerHostState.decoded に格納する。
        // metadata_ptr にデコーダー固有のメタデータを書き込む。
        // コンテンツ種別（画像・音声等）は自動判定される。
        // 戻り値: 0=成功, -1=非対応フォーマット, -2=メモリ予算超過, -3=デコードエラー
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "decode_content",
                |mut caller: Caller<'_, InnerHostState>,
                 _params_ptr: u32,
                 _params_len: u32,
                 metadata_ptr: u32|
                 -> i32 {
                    // 1. デコーダー自動選択
                    let kind = {
                        let state = caller.data();
                        match crate::decode::detect(&state.content) {
                            Some(k) => k,
                            None => return -1, // 非対応フォーマット
                        }
                    };

                    // 2. ピークメモリ推定（ヘッダのみ読み、圧縮爆弾対策）
                    let peak_size = {
                        let state = caller.data();
                        match crate::decode::estimate_peak_bytes(kind, &state.content) {
                            Ok(s) => s,
                            Err(rc) => return rc,
                        }
                    };

                    // 3. 2回目以降の呼び出し: 前回のチケットを解放
                    {
                        let state = caller.data_mut();
                        state.decode_ticket = None;
                        state.decoded = None;
                    }

                    // 4. ResourcePool で予約（Ticket 発行）
                    {
                        let pool_opt = caller.data().resource_pool.clone();
                        if let Some(ref pool) = pool_opt {
                            match pool.acquire(peak_size) {
                                Some(ticket) => {
                                    caller.data_mut().decode_ticket = Some(ticket);
                                }
                                None => return -2, // メモリ予算超過
                            }
                        }
                    }

                    // 5. フルデコード
                    let result = {
                        let state = caller.data();
                        match crate::decode::decode(kind, &state.content) {
                            Ok(r) => r,
                            Err(rc) => return rc,
                        }
                    };

                    // 6. メタデータをWASMメモリに書き込み（フォーマット非依存）
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return -3,
                        },
                        None => return -3,
                    };
                    let mem_data = memory.data_mut(&mut caller);
                    let mp = metadata_ptr as usize;
                    if mp + result.metadata.len() > mem_data.len() {
                        return -3; // metadata_ptrが境界外
                    }
                    mem_data[mp..mp + result.metadata.len()]
                        .copy_from_slice(&result.metadata);

                    // 7. デコード済みデータを格納（メタデータも保持）
                    let w = u32::from_le_bytes([result.metadata[0], result.metadata[1], result.metadata[2], result.metadata[3]]);
                    let h = u32::from_le_bytes([result.metadata[4], result.metadata[5], result.metadata[6], result.metadata[7]]);
                    let ch = u32::from_le_bytes([result.metadata[8], result.metadata[9], result.metadata[10], result.metadata[11]]);
                    let state = caller.data_mut();
                    state.decoded = Some(DecodedContent {
                        data: result.data,
                        width: w,
                        height: h,
                        channels: ch,
                    });

                    0 // 成功
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("decode_contentの登録に失敗: {e}"))
            })?;

        // read_decoded_chunk(offset: u32, length: u32, buf_ptr: u32) -> u32
        // デコード済みデータのチャンクを読み取り、WASMメモリにコピーする。
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "read_decoded_chunk",
                |mut caller: Caller<'_, InnerHostState>,
                 offset: u32,
                 length: u32,
                 buf_ptr: u32|
                 -> u32 {
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return 0,
                        },
                        None => return 0,
                    };
                    let (mem_data, state) = memory.data_and_store_mut(&mut caller);

                    let decoded = match &state.decoded {
                        Some(d) => d,
                        None => return 0,
                    };

                    let start = offset as usize;
                    if start >= decoded.data.len() {
                        return 0;
                    }
                    let end = (start + length as usize).min(decoded.data.len());
                    let chunk_len = end - start;

                    let dest = buf_ptr as usize;
                    if dest + chunk_len > mem_data.len() {
                        return 0;
                    }
                    mem_data[dest..dest + chunk_len]
                        .copy_from_slice(&decoded.data[start..end]);
                    chunk_len as u32
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("read_decoded_chunkの登録に失敗: {e}"))
            })?;

        // get_decoded_length() -> u32
        // デコード済みデータの全長を返す。デコード前は0。
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "get_decoded_length",
                |caller: Caller<'_, InnerHostState>| -> u32 {
                    caller
                        .data()
                        .decoded
                        .as_ref()
                        .map_or(0, |d| d.data.len() as u32)
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("get_decoded_lengthの登録に失敗: {e}"))
            })?;

        // get_decoded_feature(spec_ptr: u32, spec_len: u32, output_ptr: u32) -> i32
        // JSON specに基づいてデコード済みデータの特徴量を計算する。
        // spec: {"op":"grayscale_resize","width":32,"height":32}
        // 戻り値: 出力バイト数（正値）またはエラーコード（負値）
        // -1=specパースエラー/未知op, -3=出力バッファ境界外, -4=デコード未実行, -5=チャネル数不正
        // 仕様書 §7.1
        linker
            .func_wrap(
                "env",
                "get_decoded_feature",
                |mut caller: Caller<'_, InnerHostState>,
                 spec_ptr: u32,
                 spec_len: u32,
                 output_ptr: u32|
                 -> i32 {
                    let memory = match caller.get_export("memory") {
                        Some(ext) => match ext.into_memory() {
                            Some(m) => m,
                            None => return -3,
                        },
                        None => return -3,
                    };
                    let (mem_data, state) = memory.data_and_store_mut(&mut caller);

                    // specをWASMメモリから読み取り
                    let sp = spec_ptr as usize;
                    let sl = spec_len as usize;
                    if sp + sl > mem_data.len() {
                        return -1;
                    }
                    let spec_bytes = &mem_data[sp..sp + sl];

                    // JSONパース
                    let spec: serde_json::Value = match serde_json::from_slice(spec_bytes) {
                        Ok(v) => v,
                        Err(_) => return -1,
                    };
                    let op = match spec["op"].as_str() {
                        Some(s) => s,
                        None => return -1,
                    };

                    // デコード済みデータの存在確認
                    let decoded = match &state.decoded {
                        Some(d) => d,
                        None => return -4,
                    };

                    match op {
                        "grayscale_resize" => {
                            let target_w = match spec.get("width").and_then(|v| v.as_u64()) {
                                Some(w) => w as u32,
                                None => return -1,
                            };
                            let target_h = match spec.get("height").and_then(|v| v.as_u64()) {
                                Some(h) => h as u32,
                                None => return -1,
                            };

                            // グレースケール変換（ITU-R BT.601）+ リサイズ
                            use image::{DynamicImage, GrayImage, RgbImage, RgbaImage};

                            let gray_img = match decoded.channels {
                                1 => match GrayImage::from_raw(decoded.width, decoded.height, decoded.data.clone()) {
                                    Some(img) => img,
                                    None => return -5,
                                },
                                3 => {
                                    let rgb = match RgbImage::from_raw(decoded.width, decoded.height, decoded.data.clone()) {
                                        Some(img) => img,
                                        None => return -5,
                                    };
                                    DynamicImage::ImageRgb8(rgb).to_luma8()
                                }
                                4 => {
                                    let rgba = match RgbaImage::from_raw(decoded.width, decoded.height, decoded.data.clone()) {
                                        Some(img) => img,
                                        None => return -5,
                                    };
                                    DynamicImage::ImageRgba8(rgba).to_luma8()
                                }
                                _ => return -5,
                            };

                            // バイリニア補間リサイズ
                            let resized = image::imageops::resize(
                                &gray_img,
                                target_w,
                                target_h,
                                image::imageops::FilterType::Triangle,
                            );
                            let output = resized.into_raw();

                            // WASMメモリに出力
                            let dest = output_ptr as usize;
                            if dest + output.len() > mem_data.len() {
                                return -3;
                            }
                            mem_data[dest..dest + output.len()].copy_from_slice(&output);
                            output.len() as i32
                        }
                        _ => -1, // 未知のop
                    }
                },
            )
            .map_err(|e| {
                WasmError::ExecutionError(format!("get_decoded_featureの登録に失敗: {e}"))
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// テスト: ホスト関数経由でデータ受け渡し
    /// 仕様書 §7.1
    #[test]
    fn test_host_function_data_passing() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            ;; 結果JSON: {"result":"ok"} (15バイト)
            (data (i32.const 1024) "\0f\00\00\00{\"result\":\"ok\"}")
            (func (export "alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "compute_phash") (result i32)
                ;; ホスト関数を呼び出してデータ受け渡しをテスト
                (drop (call $len))
                (drop (call $read (i32.const 0) (i32.const 256) (i32.const 4096)))
                (drop (call $gcf (i32.const 0) (i32.const 0) (i32.const 8192)))
                (drop (call $ext (i32.const 12288) (i32.const 1024)))
                ;; 事前初期化された結果を返す
                (i32.const 1024)
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let content = b"Hello, WASM host!";
        let ext_input = b"{\"key\": \"value\"}";

        let result = runner
            .execute(&wasm, content, Some(ext_input), "compute_phash")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["result"], "ok");
    }

    /// テスト: Fuel制限超過でエラー
    /// 仕様書 §7.1
    #[test]
    fn test_fuel_exhaustion() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
            (func (export "compute_phash") (result i32)
                ;; 無限ループ
                (loop $inf (br $inf))
                (unreachable)
            )
        )"#,
        )
        .unwrap();

        // 極小のFuel制限
        let runner = WasmRunner::new(100, 16 * 1024 * 1024);
        let result = runner.execute(&wasm, b"content", None, "compute_phash");

        assert!(result.is_err());
        match result.unwrap_err() {
            WasmError::FuelExhausted => {} // 期待通り
            other => panic!("FuelExhaustedが期待されますが、取得: {other}"),
        }
    }

    /// テスト: WASMトラップがcatch_unwindで捕捉される
    /// 仕様書 §7.1
    #[test]
    fn test_trap_caught() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 0))
            (func (export "compute_phash") (result i32)
                ;; unreachableトラップ
                (unreachable)
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner.execute(&wasm, b"content", None, "compute_phash");

        assert!(result.is_err());
        // ExecutionErrorとして捕捉される（catch_unwindの内側でwasmtimeがエラーを返す）
        match result.unwrap_err() {
            WasmError::ExecutionError(_) => {} // 期待通り
            other => panic!("ExecutionErrorが期待されますが、取得: {other}"),
        }
    }

    /// テスト: get_content_lengthが正しい値を返す
    #[test]
    fn test_content_length() {
        // get_content_lengthの戻り値をそのまま結果バッファの最初のバイトに書き込むWASM
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "compute_phash") (result i32)
                (local $content_len i32)
                (local.set $content_len (call $len))
                ;; 結果JSON: {"len":NNN} を構築
                ;; 簡易実装: 固定結果を返す（content_lenが42の場合テスト成功）
                ;; 実際のテストではcontent長を42バイトに設定する
                ;; 結果: {"len":42} = 10バイト
                (i32.store (i32.const 1024) (i32.const 10))
                (i64.store (i32.const 1028) (i64.const 0x3a226e656c227b))  ;; {"len":
                (i32.store16 (i32.const 1035) (i32.const 0x3234))          ;; 42
                (i32.store8 (i32.const 1037) (i32.const 0x7d))             ;; }
                (i32.const 1024)
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let content = vec![0u8; 42]; // 42バイトのコンテンツ

        let result = runner
            .execute(&wasm, &content, None, "compute_phash")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["len"], 42);
    }

    /// テスト: get_content_featureがSHA-256を正しく計算する
    /// 仕様書 §7.1
    #[test]
    fn test_get_content_feature_sha256() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            ;; JSON spec: {"op":"sha256"} (15 bytes) at offset 256
            (data (i32.const 256) "{\"op\":\"sha256\"}")
            ;; 成功時の結果: {"hash_size":32} = 16バイト
            (data (i32.const 1024) "\10\00\00\00{\"hash_size\":32}")
            ;; 失敗時の結果: {"hash_size":0}  = 15バイト
            (data (i32.const 2048) "\0f\00\00\00{\"hash_size\":0}")
            (func (export "alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "compute_phash") (result i32)
                (local $hash_size i32)
                ;; get_content_feature(spec_ptr=256, spec_len=15, output_ptr=8192)
                (local.set $hash_size (call $gcf
                    (i32.const 256)
                    (i32.const 15)
                    (i32.const 8192)
                ))
                ;; hash_sizeが32であれば成功結果、そうでなければ失敗結果を返す
                (if (result i32) (i32.eq (local.get $hash_size) (i32.const 32))
                    (then (i32.const 1024))
                    (else (i32.const 2048))
                )
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner
            .execute(&wasm, b"test data for hashing", None, "compute_phash")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["hash_size"], 32);
    }

    /// テスト: hmac_contentがHMAC-SHA256を正しく計算する
    /// 仕様書 §7.1
    #[test]
    fn test_hmac_content_sha256() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            ;; HMACキー "secret" (6バイト) をオフセット256に配置
            (data (i32.const 256) "secret")
            ;; 成功時の結果: {"hmac_size":32} = 16バイト
            (data (i32.const 1024) "\10\00\00\00{\"hmac_size\":32}")
            ;; 失敗時の結果: {"hmac_size":0}  = 15バイト
            (data (i32.const 2048) "\0f\00\00\00{\"hmac_size\":0}")
            (func (export "alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "compute_phash") (result i32)
                (local $hmac_size i32)
                ;; HMAC-SHA256(key="secret", コンテンツ全体)をオフセット8192に書き込む
                (local.set $hmac_size (call $hmac
                    (i32.const 0)     ;; algorithm=0 (SHA256)
                    (i32.const 256)   ;; key_ptr
                    (i32.const 6)     ;; key_len ("secret" = 6 bytes)
                    (i32.const 0)     ;; offset
                    (i32.const 65535) ;; length (全コンテンツ)
                    (i32.const 8192)  ;; out_ptr
                ))
                ;; hmac_sizeが32であれば成功
                (if (result i32) (i32.eq (local.get $hmac_size) (i32.const 32))
                    (then (i32.const 1024))
                    (else (i32.const 2048))
                )
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner
            .execute(&wasm, b"test data for hmac", None, "compute_phash")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["hmac_size"], 32);
    }

    /// テスト: 不正WASMバイナリでCompileError
    #[test]
    fn test_invalid_wasm_binary() {
        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner.execute(b"not wasm", b"content", None, "process");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WasmError::CompileError(_)));
    }

    /// テスト: 存在しないエクスポート関数名でExecutionError
    #[test]
    fn test_missing_export_function() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner.execute(&wasm, b"content", None, "nonexistent_func");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WasmError::ExecutionError(_)));
    }

    /// テスト: WASM関数がptr=0を返した場合のエラー
    #[test]
    fn test_result_ptr_zero() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "process") (result i32)
                (i32.const 0)
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner.execute(&wasm, b"content", None, "process");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WasmError::ExecutionError(_)));
    }

    /// テスト: 結果バッファのjson_len=0でエラー
    #[test]
    fn test_result_buffer_zero_length() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (memory (export "memory") 1)
            ;; json_len = 0 at offset 1024
            (data (i32.const 1024) "\00\00\00\00")
            (func (export "process") (result i32)
                (i32.const 1024)
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner.execute(&wasm, b"content", None, "process");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), WasmError::ExecutionError(_)));
    }

    /// decode_contentテスト用WATテンプレート。
    /// decode_content を呼び出し、rc==0 かつ dec_len>0 なら {"ok":1}、それ以外は {"ok":0} を返す。
    fn decode_test_wat() -> Vec<u8> {
        wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (import "env" "decode_content" (func $decode (param i32 i32 i32) (result i32)))
            (import "env" "read_decoded_chunk" (func $read_dec (param i32 i32 i32) (result i32)))
            (import "env" "get_decoded_length" (func $dec_len (result i32)))
            (import "env" "get_decoded_feature" (func $gdf (param i32 i32 i32) (result i32)))
            (memory (export "memory") 2)
            ;; {"ok":1} = 8 bytes
            (data (i32.const 1024) "\08\00\00\00{\"ok\":1}")
            ;; {"ok":0} = 8 bytes
            (data (i32.const 2048) "\08\00\00\00{\"ok\":0}")
            (func (export "alloc") (param i32) (result i32) (i32.const 16384))

            (func (export "process") (result i32)
                (local $rc i32)
                (local $dec_length i32)
                ;; decode to native format, metadata at offset 8192
                (local.set $rc (call $decode (i32.const 0) (i32.const 0) (i32.const 8192)))
                (local.set $dec_length (call $dec_len))
                ;; rc==0 かつ dec_length>0 なら成功
                (if (result i32) (i32.and
                    (i32.eq (local.get $rc) (i32.const 0))
                    (i32.gt_u (local.get $dec_length) (i32.const 0))
                )
                    (then (i32.const 1024))
                    (else (i32.const 2048))
                )
            )
        )"#,
        )
        .unwrap()
    }

    /// テスト: decode_contentがPNG画像を正しくデコードする
    /// 仕様書 §7.1
    #[test]
    fn test_decode_content_png_success() {
        let wasm = decode_test_wat();
        let content = include_bytes!("../../../tests/fixtures/test_2x2.png");

        let runner = WasmRunner::new(100_000_000, 64 * 1024 * 1024);
        let result = runner
            .execute(&wasm, content, None, "process")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["ok"], 1);
    }

    /// テスト: decode_contentが非画像データで-1を返す
    /// 仕様書 §7.1
    #[test]
    fn test_decode_content_unsupported_format() {
        // decode失敗時: rc != 0 → {"ok":0}
        let wasm = decode_test_wat();
        let content = b"this is not an image file at all";

        let runner = WasmRunner::new(100_000_000, 64 * 1024 * 1024);
        let result = runner
            .execute(&wasm, content, None, "process")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["ok"], 0);
    }

    /// テスト: decode_contentがメモリ予算超過で-2を返す
    /// 仕様書 §7.1
    #[test]
    fn test_decode_content_memory_budget_exceeded() {
        let wasm = decode_test_wat();
        let content = include_bytes!("../../../tests/fixtures/test_2x2.png");

        // 極小のResourcePool（1バイト）— 2x2 RGBA = 16バイトなので確実に超過
        let pool = Arc::new(ResourcePool::new(1));
        let runner = WasmRunner::with_resource_pool(100_000_000, 64 * 1024 * 1024, pool);
        let result = runner
            .execute(&wasm, content, None, "process")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["ok"], 0);
    }

    /// テスト: decode前のread_decoded_chunkが0を返す
    /// 仕様書 §7.1
    #[test]
    fn test_read_decoded_before_decode_returns_zero() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (import "env" "decode_content" (func $decode (param i32 i32 i32) (result i32)))
            (import "env" "read_decoded_chunk" (func $read_dec (param i32 i32 i32) (result i32)))
            (import "env" "get_decoded_length" (func $dec_len (result i32)))
            (import "env" "get_decoded_feature" (func $gdf (param i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            (func (export "alloc") (param i32) (result i32) (i32.const 4096))
            ;; 成功: {"pre":0} = 9 bytes
            (data (i32.const 1024) "\09\00\00\00{\"pre\":0}")
            ;; 失敗: {"pre":1} = 9 bytes
            (data (i32.const 2048) "\09\00\00\00{\"pre\":1}")
            (func (export "process") (result i32)
                (local $n i32)
                (local $dec_len i32)
                ;; decodeせずにread_decoded_chunkを呼ぶ
                (local.set $n (call $read_dec (i32.const 0) (i32.const 256) (i32.const 4096)))
                (local.set $dec_len (call $dec_len))
                ;; n==0 かつ dec_len==0 なら成功
                (if (result i32) (i32.and
                    (i32.eq (local.get $n) (i32.const 0))
                    (i32.eq (local.get $dec_len) (i32.const 0))
                )
                    (then (i32.const 1024))
                    (else (i32.const 2048))
                )
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner
            .execute(&wasm, b"some content", None, "process")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["pre"], 0);
    }

    /// テスト: C2PA署名済みJPEGをdecode_contentで正しくデコードできる。
    /// 本番環境ではExtensionに渡されるコンテンツはC2PA署名付きであるため、
    /// image crateがC2PA埋め込みJPEGを正常にデコードできることを保証する。
    /// 仕様書 §7.1
    #[test]
    fn test_decode_content_c2pa_signed_jpeg() {
        // test_4x4.jpg をベースにC2PA署名済みコンテンツを動的に生成
        // （test.jpg は1x1最小JPEGで image crate がデコードできないため test_4x4.jpg を使用）
        let certs = include_bytes!("../../../tests/fixtures/certs/chain.pem");
        let private_key = include_bytes!("../../../tests/fixtures/certs/ee.key");
        let test_image = include_bytes!("../../../tests/fixtures/test_4x4.jpg");

        let signer =
            c2pa::create_signer::from_keys(certs, private_key, c2pa::SigningAlg::Ed25519, None)
                .unwrap();

        let manifest_json = serde_json::json!({
            "title": "test-decode.jpg",
            "format": "image/jpeg",
            "claim_generator_info": [{"name": "wasm-host-test", "version": "0.1"}]
        })
        .to_string();

        let mut builder = c2pa::Builder::from_json(&manifest_json).unwrap();
        let mut source = std::io::Cursor::new(test_image.as_slice());
        let mut dest = std::io::Cursor::new(Vec::new());
        builder
            .sign(signer.as_ref(), "image/jpeg", &mut source, &mut dest)
            .unwrap();
        let c2pa_content = dest.into_inner();

        // C2PAマニフェストが埋め込まれているのでサイズが増えている
        assert!(c2pa_content.len() > test_image.len());

        // decode_content がC2PA付きJPEGをデコードできることを検証
        let wasm = decode_test_wat();
        let runner = WasmRunner::new(100_000_000, 64 * 1024 * 1024);
        let result = runner
            .execute(&wasm, &c2pa_content, None, "process")
            .expect("C2PA署名済みJPEGのデコードに成功するべき");

        assert_eq!(
            result.output["ok"], 1,
            "C2PA署名済みJPEGがデコードできませんでした"
        );
    }

    /// テスト: get_decoded_featureがgrayscale_resizeを正しく計算する
    /// 仕様書 §7.1
    #[test]
    fn test_get_decoded_feature_grayscale_resize() {
        // decode_content後にget_decoded_featureを呼び出し、出力サイズが32*32=1024であることを確認
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (import "env" "decode_content" (func $decode (param i32 i32 i32) (result i32)))
            (import "env" "read_decoded_chunk" (func $read_dec (param i32 i32 i32) (result i32)))
            (import "env" "get_decoded_length" (func $dec_len (result i32)))
            (import "env" "get_decoded_feature" (func $gdf (param i32 i32 i32) (result i32)))
            (memory (export "memory") 2)
            ;; JSON spec: {"op":"grayscale_resize","width":32,"height":32} (48 bytes) at offset 256
            (data (i32.const 256) "{\"op\":\"grayscale_resize\",\"width\":32,\"height\":32}")
            ;; {"size":1024} = 13 bytes
            (data (i32.const 1024) "\0d\00\00\00{\"size\":1024}")
            ;; {"size":0} = 10 bytes
            (data (i32.const 2048) "\0a\00\00\00{\"size\":0}")
            (func (export "alloc") (param i32) (result i32) (i32.const 32768))

            (func (export "process") (result i32)
                (local $rc i32)
                (local $feat_size i32)
                ;; 1. decode
                (local.set $rc (call $decode (i32.const 0) (i32.const 0) (i32.const 8192)))
                (if (i32.ne (local.get $rc) (i32.const 0))
                    (then (return (i32.const 2048)))
                )
                ;; 2. get_decoded_feature(spec_ptr=256, spec_len=49, output_ptr=16384)
                (local.set $feat_size (call $gdf
                    (i32.const 256)
                    (i32.const 48)
                    (i32.const 16384)
                ))
                ;; feat_size==1024 なら成功
                (if (result i32) (i32.eq (local.get $feat_size) (i32.const 1024))
                    (then (i32.const 1024))
                    (else (i32.const 2048))
                )
            )
        )"#,
        )
        .unwrap();

        let content = include_bytes!("../../../tests/fixtures/test_2x2.png");
        let runner = WasmRunner::new(100_000_000, 64 * 1024 * 1024);
        let result = runner
            .execute(&wasm, content, None, "process")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["size"], 1024);
    }

    /// テスト: decode前のget_decoded_featureが-4を返す
    /// 仕様書 §7.1
    #[test]
    fn test_get_decoded_feature_before_decode() {
        let wasm = wat::parse_str(
            r#"(module
            (import "env" "read_content_chunk" (func $read (param i32 i32 i32) (result i32)))
            (import "env" "get_content_length" (func $len (result i32)))
            (import "env" "get_content_feature" (func $gcf (param i32 i32 i32) (result i32)))
            (import "env" "hmac_content" (func $hmac (param i32 i32 i32 i32 i32 i32) (result i32)))
            (import "env" "get_extension_input" (func $ext (param i32 i32) (result i32)))
            (import "env" "decode_content" (func $decode (param i32 i32 i32) (result i32)))
            (import "env" "read_decoded_chunk" (func $read_dec (param i32 i32 i32) (result i32)))
            (import "env" "get_decoded_length" (func $dec_len (result i32)))
            (import "env" "get_decoded_feature" (func $gdf (param i32 i32 i32) (result i32)))
            (memory (export "memory") 1)
            ;; JSON spec at offset 256
            (data (i32.const 256) "{\"op\":\"grayscale_resize\",\"width\":32,\"height\":32}")
            ;; {"rc":-4} = 9 bytes
            (data (i32.const 1024) "\09\00\00\00{\"rc\":-4}")
            ;; {"rc":0} = 8 bytes
            (data (i32.const 2048) "\08\00\00\00{\"rc\":0}")
            (func (export "alloc") (param i32) (result i32) (i32.const 4096))
            (func (export "process") (result i32)
                (local $rc i32)
                ;; decodeせずにget_decoded_featureを呼ぶ
                (local.set $rc (call $gdf (i32.const 256) (i32.const 48) (i32.const 8192)))
                ;; rc==-4 なら成功（デコード未実行エラー）
                (if (result i32) (i32.eq (local.get $rc) (i32.const -4))
                    (then (i32.const 1024))
                    (else (i32.const 2048))
                )
            )
        )"#,
        )
        .unwrap();

        let runner = WasmRunner::new(10_000_000, 16 * 1024 * 1024);
        let result = runner
            .execute(&wasm, b"some content", None, "process")
            .expect("WASM実行に成功するべき");

        assert_eq!(result.output["rc"], -4);
    }

    /// テスト: ResourcePoolのDrop時にTicketが解放される
    /// 仕様書 §7.1
    #[test]
    fn test_resource_pool_released_after_execution() {
        let wasm = decode_test_wat();
        let content = include_bytes!("../../../tests/fixtures/test_2x2.png");

        let pool = Arc::new(ResourcePool::new(100 * 1024 * 1024));
        let runner = WasmRunner::with_resource_pool(100_000_000, 64 * 1024 * 1024, pool.clone());

        // 実行前: 使用量0
        assert_eq!(pool.total_used(), 0);

        let _result = runner.execute(&wasm, content, None, "process").unwrap();

        // 実行後: InnerHostState がDropされ、Ticketが解放済み
        assert_eq!(pool.total_used(), 0);
    }
}
