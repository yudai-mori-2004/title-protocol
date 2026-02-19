"use strict";
/**
 * オフチェーンストレージ
 *
 * 仕様書 §5.1 Step 7: signed_jsonをオフチェーンストレージにアップロードする。
 * CoreはArweave（永続保存が必須）、Extensionは任意のストレージを使用可能。
 */
Object.defineProperty(exports, "__esModule", { value: true });
exports.ArweaveStorage = void 0;
/** Arweave (Irys経由) ストレージプロバイダ。Core用。 */
class ArweaveStorage {
    _gateway;
    _token;
    constructor(_gateway = "https://node2.irys.xyz", _token = "solana") {
        this._gateway = _gateway;
        this._token = _token;
    }
    async upload(_data, _contentType) {
        // TODO: Irys SDKを使用したArweaveアップロード
        throw new Error("Not implemented");
    }
}
exports.ArweaveStorage = ArweaveStorage;
