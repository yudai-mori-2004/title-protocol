/**
 * Title Protocol 共有型定義 (TypeScript)
 *
 * crates/types と対応するTypeScript型定義。
 * 仕様書 §5
 */

// ---------------------------------------------------------------------------
// signed_json 構造 (仕様書 §5.1 Step 4, Step 5)
// ---------------------------------------------------------------------------

/** TEEが生成する署名済みJSON。仕様書 §5.1 Step 4 */
export interface SignedJson {
  protocol: string;
  tee_type: string;
  /** Base58エンコードされたEd25519公開鍵 */
  tee_pubkey: string;
  /** Base64エンコードされた署名 */
  tee_signature: string;
  /** Base64エンコードされたAttestation Document */
  tee_attestation: string;
  payload: CorePayload | ExtensionPayload;
  attributes: Attribute[];
}

// ---------------------------------------------------------------------------
// Payload (仕様書 §5.1 Step 4, Step 5)
// ---------------------------------------------------------------------------

/** Core用ペイロード。仕様書 §5.1 Step 4 */
export interface CorePayload {
  content_hash: string;
  content_type: string;
  /** Base58エンコードされたウォレットアドレス */
  creator_wallet: string;
  tsa_timestamp?: number;
  tsa_pubkey_hash?: string;
  /** Base64エンコードされたRFC 3161トークン */
  tsa_token_data?: string;
  nodes: GraphNode[];
  links: GraphLink[];
}

/** Extension用ペイロード。仕様書 §5.1 Step 5 */
export interface ExtensionPayload {
  content_hash: string;
  content_type: string;
  creator_wallet: string;
  extension_id: string;
  wasm_source: string;
  wasm_hash: string;
  extension_input_hash?: string;
  [key: string]: unknown;
}

// ---------------------------------------------------------------------------
// 来歴グラフ (仕様書 §2.2)
// ---------------------------------------------------------------------------

/** 来歴グラフのノード。仕様書 §2.2 */
export interface GraphNode {
  id: string;
  type: "final" | "ingredient";
}

/** 来歴グラフのリンク。仕様書 §2.2 */
export interface GraphLink {
  source: string;
  target: string;
  role: string;
}

// ---------------------------------------------------------------------------
// Global Config (仕様書 §5.2 Step 1)
// ---------------------------------------------------------------------------

/** Global Config PDA。仕様書 §5.2 Step 1 */
export interface GlobalConfig {
  authority: string;
  core_collection_mint: string;
  ext_collection_mint: string;
  trusted_tee_nodes: TrustedTeeNode[];
  trusted_tsa_keys: string[];
  trusted_wasm_modules: TrustedWasmModule[];
}

/** 信頼されたTEEノード情報。仕様書 §5.2 Step 1 */
export interface TrustedTeeNode {
  /** Base58エンコードされたEd25519署名用公開鍵 */
  signing_pubkey: string;
  /** Base64エンコードされたX25519暗号化用公開鍵 */
  encryption_pubkey: string;
  encryption_algorithm: string;
  /** Base58エンコードされたGateway署名用公開鍵 */
  gateway_pubkey: string;
  gateway_endpoint: string;
  status: string;
  tee_type: string;
  expected_measurements: ExpectedMeasurements;
}

/** TEEの期待される測定値。仕様書 §5.2 Step 1 */
export interface ExpectedMeasurements {
  pcr0?: string;
  pcr1?: string;
  pcr2?: string;
  measurement?: string;
  mrtd?: string;
  rtmr0?: string;
  rtmr1?: string;
  rtmr2?: string;
  rtmr3?: string;
}

/** 信頼されたWASMモジュール。仕様書 §5.2 Step 1 */
export interface TrustedWasmModule {
  extension_id: string;
  wasm_source: string;
  wasm_hash: string;
}

// ---------------------------------------------------------------------------
// 暗号化ペイロード (仕様書 §5.1 Step 2)
// ---------------------------------------------------------------------------

/** 暗号化されたペイロード。仕様書 §5.1 Step 2 */
export interface EncryptedPayload {
  /** Base64エンコードされたX25519公開鍵 */
  ephemeral_pubkey: string;
  /** Base64エンコードされたAES-GCM nonce */
  nonce: string;
  /** Base64エンコードされた暗号文 */
  ciphertext: string;
}

/** クライアントが構築するペイロード（暗号化前）。仕様書 §5.1 Step 1 */
export interface ClientPayload {
  /** Base58エンコードされたSolanaウォレットアドレス */
  owner_wallet: string;
  /** Base64エンコードされたコンテンツバイナリ */
  content: string;
  /** Base64エンコードされた.c2paファイル（Optional） */
  sidecar_manifest?: string;
  extension_inputs?: Record<string, unknown>;
}

// ---------------------------------------------------------------------------
// API リクエスト/レスポンス (仕様書 §6.2)
// ---------------------------------------------------------------------------

/** /verify リクエスト。仕様書 §5.1 Step 3 */
export interface VerifyRequest {
  download_url: string;
  processor_ids: string[];
}

/** /verify レスポンス。仕様書 §5.1 Step 6 */
export interface VerifyResponse {
  results: ProcessorResult[];
}

/** プロセッサ結果 */
export interface ProcessorResult {
  processor_id: string;
  signed_json: SignedJson;
}

/** /sign リクエスト。仕様書 §5.1 Step 8 */
export interface SignRequest {
  recent_blockhash: string;
  requests: SignRequestItem[];
}

/** /sign リクエストアイテム */
export interface SignRequestItem {
  signed_json_uri: string;
}

/** /sign レスポンス。仕様書 §5.1 Step 10 */
export interface SignResponse {
  partial_txs: string[];
}

// ---------------------------------------------------------------------------
// cNFT メタデータ (仕様書 §5.1 Step 11)
// ---------------------------------------------------------------------------

/** cNFTオンチェーンメタデータ。仕様書 §5.1 Step 11 */
export interface CnftMetadata {
  name: string;
  symbol: string;
  uri: string;
  attributes: Attribute[];
}

/** Metaplex標準属性 */
export interface Attribute {
  trait_type: string;
  value: string;
}

// ---------------------------------------------------------------------------
// ノード情報 (仕様書 §6.2)
// ---------------------------------------------------------------------------

/** /.well-known/title-node-info レスポンス。仕様書 §6.2 */
export interface NodeInfo {
  signing_pubkey: string;
  supported_extensions: string[];
  limits: {
    max_single_content_bytes: number;
    max_concurrent_bytes: number;
  };
}

// ---------------------------------------------------------------------------
// SDK固有の型
// ---------------------------------------------------------------------------

/** resolve() の結果 */
export interface ResolveResult {
  owner: string | null;
  provenanceGraph: {
    nodes: GraphNode[];
    links: GraphLink[];
  };
  extensions: ResolvedExtension[];
}

/** 解決済みExtension */
export interface ResolvedExtension {
  extensionId: string;
  data: Record<string, unknown>;
}
