/**
 * E2EEクライアント側の暗号処理
 *
 * 仕様書 §6.4 ハイブリッド暗号化
 *
 * - エフェメラルX25519キーペア生成
 * - ECDH鍵交換
 * - HKDF-SHA256鍵導出
 * - AES-256-GCM暗号化/復号
 */
import type { EncryptedPayload } from "./types";
/** エフェメラルキーペア（X25519） */
export interface EphemeralKeyPair {
    publicKey: Uint8Array;
    secretKey: Uint8Array;
}
/**
 * エフェメラルX25519キーペアを生成する。
 * 仕様書 §6.4 Step 2
 */
export declare function generateEphemeralKeyPair(): EphemeralKeyPair;
/**
 * ECDH鍵交換で共有秘密を導出する。
 * 仕様書 §6.4 Step 3
 */
export declare function deriveSharedSecret(_ephemeralSecretKey: Uint8Array, _teePublicKey: Uint8Array): Uint8Array;
/**
 * HKDF-SHA256で対称鍵を導出する。
 * 仕様書 §6.4 Step 4
 */
export declare function deriveSymmetricKey(_sharedSecret: Uint8Array): Uint8Array;
/**
 * AES-256-GCMでペイロードを暗号化する。
 * 仕様書 §6.4 Step 4
 */
export declare function encrypt(_symmetricKey: Uint8Array, _plaintext: Uint8Array): EncryptedPayload;
/**
 * AES-256-GCMで暗号文を復号する。
 * 仕様書 §6.4 Step 9
 */
export declare function decrypt(_symmetricKey: Uint8Array, _nonce: Uint8Array, _ciphertext: Uint8Array): Uint8Array;
