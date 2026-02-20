/**
 * register() 関数
 *
 * 仕様書 §6.7: コンテンツの検証・メタデータ保存・cNFT発行を実行する。
 *
 * 内部処理フロー（11ステップ）:
 * 1. TEEノードの選択（暗号化公開鍵の取得）
 * 2. エフェメラルキーペア生成
 * 3. ClientPayload構築
 * 4. ペイロード暗号化（ECDH → HKDF → AES-GCM）
 * 5. Temporary Storageにアップロード
 * 6. /verify 呼び出し
 * 7. レスポンス復号（エフェメラル秘密鍵 + 共通鍵）
 * 8. wasm_hash検証（セキュリティクリティカル — 仕様書 §6.4）
 * 9. signed_jsonをオフチェーンストレージにアップロード
 * 10. /sign 呼び出し
 * 11. partial_tx検証、ウォレット署名、ブロードキャスト
 */

import type {
  RegisterResult,
  ContentResult,
  ExtensionResult,
  ClientPayload,
  VerifyResponse,
  SignedJson,
  ExtensionPayload,
} from "./types";
import { TitleClient, TeeSession } from "./client";
import { encryptPayload, decryptResponse } from "./crypto";

// ---------------------------------------------------------------------------
// オプション
// ---------------------------------------------------------------------------

/** register() のオプション */
export interface RegisterOptions {
  /** コンテンツバイナリ */
  content: Uint8Array;
  /** コンテンツのMIMEタイプ */
  contentType: string;
  /**
   * ウォレットアダプタ。
   * Solana wallet-adapter 互換: publicKey.toBase58() と signTransaction(tx) を持つ。
   */
  owner: {
    publicKey: { toBase58(): string };
    signTransaction(tx: unknown): Promise<unknown>;
  };
  /**
   * TEEセッション。client.selectNode() で取得する。
   * 暗号化アップロード〜verify〜signまで同一ノードへのアフィニティを保証する。
   * 省略時はclientがランダムにノードを選択する。
   */
  session?: TeeSession;
  /** 実行するプロセッサIDリスト（例: ["core-c2pa", "phash-v1"]） */
  processorIds: string[];
  /** Extension補助入力（Optional） */
  extensionInputs?: Record<string, unknown>;
  /** Gateway代行ミントを使用するか */
  delegateMint?: boolean;
  /** サイドカーマニフェスト .c2pa ファイル（Optional） */
  sidecarManifest?: Uint8Array;
}

// ---------------------------------------------------------------------------
// register()
// ---------------------------------------------------------------------------

/**
 * コンテンツの登録を実行する。
 * 仕様書 §6.7
 *
 * @param client - TitleClient インスタンス
 * @param options - 登録オプション
 */
export async function register(
  client: TitleClient,
  options: RegisterOptions
): Promise<RegisterResult> {
  const {
    content,
    contentType,
    owner,
    processorIds,
    extensionInputs,
    delegateMint = false,
    sidecarManifest,
  } = options;

  // --- Step 1: TEEノード選択（セッションアフィニティ） ---
  // 既にセッションがある場合はそのノードを使う（暗号化時にTEEが確定するため）
  const session = options.session ?? (await client.selectNode());

  // TEEのX25519暗号化公開鍵をBase64デコード
  const teeEncryptionPubkey = Buffer.from(session.encryptionPubkey, "base64");

  // --- Step 2-3: ClientPayload構築 ---
  const clientPayload: ClientPayload = {
    owner_wallet: owner.publicKey.toBase58(),
    content: Buffer.from(content).toString("base64"),
    ...(sidecarManifest && {
      sidecar_manifest: Buffer.from(sidecarManifest).toString("base64"),
    }),
    ...(extensionInputs && { extension_inputs: extensionInputs }),
  };

  // --- Step 4: ペイロード暗号化（ECDH → HKDF → AES-GCM）---
  const payloadBytes = new TextEncoder().encode(JSON.stringify(clientPayload));
  const { symmetricKey, encryptedPayload } = await encryptPayload(
    teeEncryptionPubkey,
    payloadBytes
  );

  // --- Step 5: Temporary Storageにアップロード ---
  const { downloadUrl } = await client.upload(
    session.gatewayUrl,
    encryptedPayload
  );

  // --- Step 6: /verify 呼び出し ---
  const encryptedResponse = await client.verify(session.gatewayUrl, {
    download_url: downloadUrl,
    processor_ids: processorIds,
  });

  // --- Step 7: レスポンス復号 ---
  const decryptedBytes = await decryptResponse(
    symmetricKey,
    encryptedResponse.nonce,
    encryptedResponse.ciphertext
  );
  const verifyResponse: VerifyResponse = JSON.parse(
    new TextDecoder().decode(decryptedBytes)
  );

  // --- Step 8: wasm_hash検証（セキュリティクリティカル）---
  // Extension signed_jsonに含まれるwasm_hashを、GlobalConfigのtrusted_wasm_modulesと照合する。
  // これにより、不正なWASMモジュールがTEE内で実行されていないことをクライアント側で検証する。
  // 仕様書 §6.4 レスポンス暗号化の二層防御
  const trustedModules = client.getTrustedWasmModules();
  for (const result of verifyResponse.results) {
    if (result.processor_id === "core-c2pa") continue; // CoreはWASM検証不要

    const signedJson = result.signed_json as unknown as SignedJson;
    const payload = signedJson.payload as ExtensionPayload;

    if (!payload.wasm_hash) {
      throw new Error(
        `Extension ${result.processor_id} の signed_json に wasm_hash が含まれていません`
      );
    }

    const trusted = trustedModules.find(
      (m) => m.extension_id === result.processor_id
    );
    if (!trusted) {
      throw new Error(
        `Extension ${result.processor_id} はGlobalConfigのtrusted_wasm_modulesに登録されていません`
      );
    }
    if (trusted.wasm_hash !== payload.wasm_hash) {
      throw new Error(
        `Extension ${result.processor_id} のwasm_hashが不一致: ` +
          `期待値=${trusted.wasm_hash}, 実際=${payload.wasm_hash}`
      );
    }
  }

  // --- Step 9: signed_jsonをオフチェーンストレージにアップロード ---
  const contents: ContentResult[] = [];
  const signedJsonUris: string[] = [];

  let coreResult: ContentResult | null = null;

  for (const result of verifyResponse.results) {
    const jsonBytes = new TextEncoder().encode(
      JSON.stringify(result.signed_json)
    );
    const uri = await client.config.storage.upload(
      jsonBytes,
      "application/json"
    );
    signedJsonUris.push(uri);

    if (result.processor_id === "core-c2pa") {
      const signedJson = result.signed_json as unknown as SignedJson;
      const payload = signedJson.payload as { content_hash?: string };
      coreResult = {
        contentHash: payload.content_hash ?? "",
        storageUri: uri,
        extensions: [],
      };
    } else {
      // Extension結果をcore結果にぶら下げる
      if (coreResult) {
        coreResult.extensions.push({
          extensionId: result.processor_id,
          storageUri: uri,
        });
      }
    }
  }

  if (coreResult) {
    contents.push(coreResult);
  }

  // --- Step 10: /sign 呼び出し ---
  // @solana/web3.js から最新blockhashを取得
  const { Connection } = await import("@solana/web3.js");
  const connection = new Connection(client.config.solanaRpcUrl);
  const { blockhash } = await connection.getLatestBlockhash();

  const signRequest = {
    recent_blockhash: blockhash,
    requests: signedJsonUris.map((uri) => ({ signed_json_uri: uri })),
  };

  if (delegateMint) {
    // Gateway代行ミント: TEE署名 + Gateway署名 + ブロードキャストをGatewayが代行
    const { txSignatures } = await client.signAndMint(
      session.gatewayUrl,
      signRequest
    );
    return {
      contents,
      txSignatures,
    };
  }

  // 通常フロー: クライアントがウォレット署名してブロードキャスト
  const signResponse = await client.sign(session.gatewayUrl, signRequest);

  // --- Step 11: partial_tx検証 → ウォレット署名 → ブロードキャスト ---
  const { Transaction } = await import("@solana/web3.js");
  const txSignatures: string[] = [];

  for (const partialTxB64 of signResponse.partial_txs) {
    const txBytes = Buffer.from(partialTxB64, "base64");
    const tx = Transaction.from(txBytes);

    // トランザクション検証: 署名者がGlobalConfigの信頼リストに含まれるか
    // （部分署名済みなので、TEEの公開鍵が署名者に含まれているはず）
    const trustedNodes = client.getTrustedTeeNodes();
    const trustedPubkeys = new Set(trustedNodes.map((n) => n.signing_pubkey));
    const hasTrustedSigner = tx.signatures.some(
      (sig) =>
        sig.publicKey && trustedPubkeys.has(sig.publicKey.toBase58())
    );
    if (!hasTrustedSigner) {
      throw new Error(
        "partial_txに信頼されたTEEノードの署名が含まれていません"
      );
    }

    // ウォレットで署名
    const signedTx = await owner.signTransaction(tx);

    // Solanaにブロードキャスト
    // signedTx は Transaction オブジェクト
    const rawTx = (signedTx as typeof tx).serialize();
    const sig = await connection.sendRawTransaction(rawTx);
    await connection.confirmTransaction(sig);
    txSignatures.push(sig);
  }

  return {
    contents,
    partialTxs: signResponse.partial_txs,
    txSignatures,
  };
}
