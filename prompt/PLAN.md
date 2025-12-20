要点（MVP→v1→v1.5）

* MVP：①増分キャッシュ（read/list/grepの“決定的”結果だけ）②セマンティック索引（手動build + /search）を「coreに最小追加」で出す
* v1：索引を自動更新（差分ビルド）＋「生成前の自動リトリーブ注入」＋TUIにヒット率/鮮度表示
* v1.5：大規模repo最適化（優先度付きクロール、分割index、gitブランチ対応、説明可能リトリーブ）

前提（リポジトリ構造に合わせる）

* 本体はRust実装の `codex-rs/`（`core/`, `cli/`, `tui/`, `exec/` 等）で、旧TS実装は `codex-cli/`（レガシー）。([Zenn][1])
* CLI引数解析は `codex-rs/cli/src/main.rs`、中核は `codex-rs/core/`（例：`src/codex.rs`, `src/mcp_connection_manager.rs`）という整理で進める。([Zenn][1])
* ファイル系ツールの実装位置（例）：`read_file` / `list_dir` / `grep_files` は `codex-rs/core/src/tools/handlers/*` と `tools/spec.rs` に紐づく。([Qiita][2])
* 設定は `~/.codex/config.toml` が基本で、CLIの `-c key=value` で上書き可能。([GitHub][3])

---

## 実装計画（PR単位で：ファイル/モジュール、データスキーマ、コマンドUIまで）

### MVP（最短で価値が見える：キャッシュ + 手動索引 + /search）

#### PR-0：土台（設定・保存場所・テレメトリ枠）

* 状態：完了（branch: pr-0-foundation / tests: cargo test -p codex-core --lib cache::config, semantic::config, telemetry::tests, config::tests）
* 目的：以降のPRで壊れない“置き場”を先に作る（feature flag / config / ストレージ抽象）
* 追加/変更（例）

  * `codex-rs/core/src/cache/mod.rs`
  * `codex-rs/core/src/cache/config.rs`
  * `codex-rs/core/src/cache/store.rs`（KV/SQLite抽象：後で差し替え可能に）
  * `codex-rs/core/src/semantic/mod.rs`
  * `codex-rs/core/src/semantic/config.rs`
  * `codex-rs/core/src/telemetry/mod.rs`（cache hit率などの内部メトリクス）
  * `codex-rs/common/src/config_override.rs` 経由で読めるconfigキーを追加（既存の `-c` 適用に乗せる）([Zenn][1])
* データスキーマ（このPRでは“予約”だけ）

  * cache：`~/.codex/cache/` 配下（ファイル or sqlite）※パスはconfigで変更可能
  * semantic：`<workspace>/.codex-index/`（デフォはworkspaceローカル。グローバル共有はv1.5）
* コマンドUI：まだ出さない（内部フラグのみ）
* 備考：Rustログは `RUST_LOG` が効く前提でデバッグできるようにログカテゴリを切る ([GitHub][4])

#### PR-1：増分キャッシュ（read_file / list_dir のみ）

* 目的：体感速度が出る“安全なキャッシュ”から入る（副作用がない）
* 変更対象

  * `codex-rs/core/src/tools/handlers/read_file.rs`（入口でキャッシュ参照→ミスなら既存処理→保存）([Qiita][2])
  * `codex-rs/core/src/tools/handlers/list_dir.rs` 同上 ([Qiita][2])
  * `codex-rs/core/src/tools/spec.rs` は変更不要（挙動は同じ、内部のみ）
* キャッシュキー（MVP仕様）

  * `tool_name` + 正規化args（canonical json）+ `workspace_root` + 対象パスの `(mtime,size)` をSHA256
  * `read_file`：`file_path, offset, limit, mode, indentation` + ファイルstat
  * `list_dir`：`dir_path, depth, offset, limit` + ディレクトリstat（＋必要なら子のmtime集計はv1）
* 期限/破棄

  * デフォTTL：60s（configで変更）
  * LRU上限：例 256MB（config）
* コマンドUI（MVPで最低限）

  * `codex cache status`：件数/サイズ/hit率（直近N分）
  * `codex cache clear`：全消し（確認プロンプトは既存流儀に合わせる）
* 追加/変更ファイル

  * `codex-rs/cli/src/cache_cmd.rs`（サブコマンド追加）
  * `codex-rs/cli/src/main.rs` にサブコマンド配線 ([Zenn][1])

#### PR-2：増分キャッシュ（grep_files 追加）

* 目的：grepは会話中に頻発しやすいので効果が大きい（ただし無効化しやすく）
* 変更対象

  * `codex-rs/core/src/tools/handlers/grep_files.rs` ([Qiita][2])
* キャッシュキー（MVPの割り切り）

  * `pattern/include/path/limit` + `workspace_root`
  * “repo状態”の簡易指標：`.git/HEAD` の参照先 + `index` のmtime（取れるときだけ）
  * 取れない場合は短TTL（例：10s）に落とす
* コマンドUI

  * `codex cache status --by-tool`：read/list/grep の内訳

#### PR-3：セマンティック索引（手動build・最小検索API）

* 目的：まず「索引を作る→検索できる」を独立コマンドで成立させる（生成注入はまだ）
* 追加/変更

  * `codex-rs/core/src/semantic/index.rs`（チャンク化、メタ管理）
  * `codex-rs/core/src/semantic/embedding.rs`（OpenAI Embeddings呼び出し or 既存client再利用）
  * `codex-rs/core/src/semantic/vector_store.rs`（sqlite + blob、またはsled）
  * `codex-rs/cli/src/index_cmd.rs`
  * `codex-rs/cli/src/main.rs` に `codex index build|stats|clear` を追加 ([Zenn][1])
* 索引スキーマ（MVP）

  * `meta`：`schema_version, embedding_model, dim, chunk_size, created_at, workspace_fingerprint`
  * `files`：`path, content_hash(sha256), mtime, size`
  * `chunks`：`file_path, chunk_id, start_line, end_line, text_hash, embedding(blob), updated_at`
* コマンドUI（MVP）

  * `codex index build`：初回フルビルド
  * `codex index stats`：ファイル数/チャンク数/最終更新/モデル名
  * `codex index clear`

#### PR-4：/search（スラッシュコマンド）＋ CLI `codex search`

* 目的：TUIユーザーにも価値が見える形で“セマンティック検索”を露出
* 実装方針

  * まずは `codex search "<query>" --topk 10` を安定提供
  * TUIは `/search <query>` で同じ結果を表示（スラッシュコマンド機構に乗せる）([OpenAI Developers][5])
* 変更対象（例）

  * `codex-rs/cli/src/search_cmd.rs`
  * `codex-rs/tui/src/...`（スラッシュコマンドのルーティング部に追加）
* 出力形式

  * `<file>:<line_range> score=...` + 抜粋（N行）
  * `--json`（`codex exec` の `--experimental-json` と同系統の思想で）([Zenn][1])

---

### v1（自動更新 + “生成前に拾って注入” + TUI可視化）

#### PR-5：索引の差分更新（incremental indexer）

* 目的：`build` じゃなく “update” が日常になる
* 変更

  * `codex-rs/core/src/semantic/watcher.rs`（ポーリングで開始：OS依存watchはv1.5でも可）
  * `codex index update`（変更ファイルのみ再チャンク→再embedding）
* スキーマ追加

  * `files.last_indexed_at`
  * `chunks.embedding_model`（将来モデル変更に備える）

#### PR-6：リトリーブ注入（会話エンジンに接続）

* 目的：ユーザーが `/search` しなくても、Codexが“勝手に”関連コードを拾って精度が上がる
* 変更ポイント

  * `codex-rs/core/src/codex.rs` の「モデル呼び出し直前」フックを追加（topK取得→“コンテキストメッセージ”として差し込む）([Zenn][1])
* ガード

  * 予算：最大注入トークン数（config）
  * セキュリティ：sandbox/approvalは既存のファイル読みと同等に扱う（索引が古い場合はread_fileで検証してから注入、もしくは“参考情報”として注記）

#### PR-7：TUIのステータス表示

* 目的：ブラックボックス感を消す（バズり要素）
* 表示案（右上など）

  * `IDX: fresh|stale`（最終更新からの経過）
  * `CACHE: hit 42%`（直近）
* 実装

  * `codex-rs/tui/src/app/...`（既存コンポーネント構造に合わせる）([Zenn][1])

---

### v1.5（大規模・現場運用向けの差別化）

#### PR-8：gitブランチ/コミット対応の索引分岐 + 共有キャッシュ

* 目的：monorepo/複数プロジェクトで破綻しない
* 変更

  * workspace fingerprintに `git HEAD` を組み込み、ブランチが変わったら別indexに分岐
  * グローバル共有：`~/.codex/indexes/<fingerprint>/` へ（configで切替）
* 追加コマンド

  * `codex index gc`（古いfingerprintを掃除）
  * `codex cache prune --ttl 7d`

#### PR-9：説明可能リトリーブ（why this snippet?）

* 目的：レビューで強い（“なぜそれを拾ったか”が出る）
* 出力

  * `query terms` / `embedding score` / `file recency` を簡潔に表示
  * TUIで展開表示

---

## 追加する設定キー（config.toml / -c 上書き前提）

* 既存の `~/.codex/config.toml` と `-c key=value` の流儀に合わせる ([GitHub][3])
  例：
* `[cache]`

  * `enabled=true`
  * `dir="~/.codex/cache"`
  * `max_bytes=268435456`
  * `default_ttl_sec=60`
  * `tool_ttl_sec.read_file=300`
  * `tool_ttl_sec.grep_files=10`
* `[semantic_index]`

  * `enabled=true`
  * `dir=".codex-index"`（workspace相対）
  * `embedding_model="text-embedding-3-small"`（デフォ）
  * `chunk.max_lines=120`
  * `retrieve.top_k=8`
  * `retrieve.max_chars=12000`

---

## 最初のPR（PR-0）の“コミット粒度”の提案

* Commit A：`core/cache` と `core/semantic` の空モジュール + config struct
* Commit B：`cli` に feature flag を通す（まだUIなし）
* Commit C：ログカテゴリ追加（`RUST_LOG=codex_cache=debug,...` で追える）([GitHub][4])

この計画は `codex-rs` の既存分割（`cli`, `core`, `tui`, `exec`）に沿っており、`read_file/list_dir/grep_files` が `tools/handlers` で提供されている前提で“差し込みやすい順”に並べています。([Qiita][2])

[1]: https://zenn.dev/taka000/articles/19686260d6aa73 "Codex CLI 完全ガイド：全体像"
[2]: https://qiita.com/nogataka/items/d2b34e8c173fa513ac57 "Codexのエージェントが使っているツールって知ってますか？ #AI - Qiita"
[3]: https://github.com/openai/codex?utm_source=chatgpt.com "openai/codex: Lightweight coding agent that runs in your ..."
[4]: https://raw.githubusercontent.com/openai/codex/main/docs/advanced.md?utm_source=chatgpt.com "Tracing / verbose logging - GitHub"
[5]: https://developers.openai.com/codex/guides/slash-commands/?utm_source=chatgpt.com "Slash commands in Codex CLI"
