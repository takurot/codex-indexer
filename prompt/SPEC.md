要点（このForkで実現する価値）

* 初回だけ全走査し、以降はファイル変更を監視して「ツリーキャッシュ」と「索引」を増分更新する（起動・探索が体感で速くなる）。
* 文字列検索（grep/rg）と意味検索（embedding）を融合した「ハイブリッド検索」をCodex CLIに組み込み、巨大リポでも目的のコードに最短で到達できる。
* `~/.codex/config.toml` を拡張し、既存のCodex CLI設定・運用（AGENTS.md/スラッシュコマンド等）に自然に統合する。([OpenAI Developers][1])

---

# 機能仕様書：Codex CLI Fork（増分キャッシュ＋セマンティック索引）

## 1. 文書情報

* 対象：openai/codex（Codex CLI）をForkした派生CLI
* 目的：大規模コードベースでの「探索の待ち」を削減し、検索→閲覧→修正のサイクルを高速化
* 互換性方針：既存のCLI操作・設定ファイル（`~/.codex/config.toml`）を壊さず拡張する（CLIは同設定を継承する）([OpenAI Developers][2])

## 2. 背景（狙い）

* 公式リポジトリでは「セマンティックなコードベース索引と検索」を求める要望が継続的に出ており、大規模プロジェクトでの精度・速度改善が期待されている。([GitHub][3])
* `.codexignore` のような除外機構についても要望・不具合報告があり、インデックス対象の制御は実運用上の重要項目。([GitHub][4])

## 3. ゴール / 非ゴール

### 3.1 ゴール（KPI例）

* 起動後の初回ツリー取得：キャッシュ有りで 200ms〜1s 以内（リポサイズ依存）
* 検索レイテンシ：ハイブリッド検索で 300ms〜2s 以内（索引済み前提）
* 増分更新：ファイル変更から索引更新まで 1〜5秒以内（バッチ設定により調整）
* 「検索→該当ファイルを開く」までの操作数：2アクション以内（例：`/search`→候補選択）

### 3.2 非ゴール

* LSP置換（完全なIDE級のシンボル解決）を必須としない（段階導入）
* クラウド常駐インデクサ必須化はしない（ローカル完結をデフォルト）

## 4. 想定ユーザーと利用シナリオ

* 大規模モノレポ利用者：ファイル数が多く、目的箇所に辿り着くまでが遅い
* 複数言語リポの利用者：命名や構成が多様で、grepだけだと取りこぼす
* セキュリティ重視：除外設定（秘密情報・生成物）を確実に効かせたい

シナリオ例

1. 初回：`codex` 起動 → 自動で「ツリーキャッシュ作成」と「索引作成ガイド」提示
2. 日常：`/search "認証トークンの更新ロジック"` → 候補にジャンプ → 変更
3. 更新：ファイル保存→自動で差分索引更新 → 次の検索に即反映

## 5. 全体アーキテクチャ

主要コンポーネント

1. Workspace Scanner：ディレクトリ走査、ファイルメタ収集
2. Change Detector：ファイル変更検知（OS watcher + フォールバック）
3. Cache Store：ツリーキャッシュ／チャンクキャッシュを永続化
4. Chunker：ファイルを意味単位に分割（関数/クラス/段落など）
5. Embedding Backend：embedding生成（ローカル or API、バッチ対応）
6. Vector Store：ベクタDB（デフォルト：SQLite+FAISS/HNSWなどの組み込み実装、または外部）
7. Hybrid Retriever：lexical（rg）＋semantic（vector）＋再ランキング
8. CLI Integration：スラッシュコマンド・表示UI・結果のジャンプ

   * Codex CLIはスラッシュコマンドで操作を拡張できるため、本Forkでもここを主導線にする。([OpenAI Developers][5])

## 6. 永続データ仕様

### 6.1 キャッシュ格納場所

* デフォルト：`~/.codex/cache/` 配下（ユーザー設定で変更可）
* リポ識別：`repo_fingerprint = hash(abs_path + git_remote + git_head)` など

### 6.2 ツリーキャッシュ（tree cache）

目的：起動時・探索時の「全ディレクトリ列挙」を省略

データモデル（例：`tree_cache.jsonl` or `tree_cache.sqlite`）

* `path`：相対パス
* `type`：file/dir
* `size_bytes`
* `mtime_ns`
* `mode`（権限）
* `content_hash`（任意：高速差分用）
* `lang_hint`（拡張子/シェバン/簡易判定）
* `ignored_reason`（.codexignore/設定/サイズ超過など）
* `last_indexed_at`

機能要件

* 初回構築：全走査で作成
* 更新：Change Detector からのイベントで増分更新
* 整合性：異常終了時に壊れない（スナップショット/ジャーナル/原子的rename）

### 6.3 チャンクキャッシュ（chunk cache）

目的：embedding更新を「変更箇所だけ」に限定し、コストと待ち時間を削減

データモデル（例：`chunks.sqlite`）

* `chunk_id`（stable id：path + range + hash）
* `path`
* `byte_start`, `byte_end`（または行範囲）
* `chunk_hash`（内容ハッシュ）
* `symbols`（任意：関数名/クラス名/エクスポート）
* `summary`（任意：短い要約。再ランキングに使用）

更新ポリシー

* ファイル変更→該当ファイルのチャンク再分割→hash比較→差分chunkのみ再embedding

### 6.4 ベクタ索引（semantic index）

データモデル（例：`vectors.sqlite` + `vectors.bin`）

* `chunk_id`（FK）
* `embedding`（ベクタ）
* `embedding_model_id`
* `created_at`
* `quality_flags`（言語判定失敗、巨大チャンク等）

## 7. 除外（ignore）仕様

目的：安全（秘密情報の混入防止）と性能（不要な巨大ディレクトリ排除）

対象

* `.codexignore`（プロジェクトルート）

  * 公式リポで要望・不具合報告があるため、本Forkでは「必ず尊重」する方針。([GitHub][4])
* `~/.codex/config.toml` の ignore 設定（グローバル）
* `.gitignore`（任意：デフォルトON/OFFを設定可能）
* 追加：`AGENTS.md` による指示（例：`/tmp`や生成物は読まない等）

  * CodexはAGENTS.mdの読み込み規約を持つ（優先順・fallback名など）。([OpenAI Developers][6])

除外ルール（推奨デフォルト例）

* `node_modules/`, `dist/`, `build/`, `.git/`, `*.log`, `*.tmp`, `**/.env*`, `**/*secret*`
* 最大ファイルサイズ（例：2MB）超は索引対象外（オプションで変更可）
* バイナリ判定（NUL含有・mimetype等）で除外

仕様要件

* ツリーキャッシュ作成・チャンク生成・semantic index の全段階で同一の除外ロジックを適用
* `/status` に「除外統計（件数・上位ディレクトリ）」を表示

## 8. 検索仕様（ハイブリッド検索）

### 8.1 検索の種類

1. Lexical（精密）：ripgrep相当（正規表現/単語一致）
2. Semantic（近似）：embeddingで近いチャンクを探索
3. Hybrid：上記を融合し、最終候補を再ランキング

### 8.2 ハイブリッド融合（例）

* `score = α * semantic + β * lexical + γ * priors`

  * priors：最近編集したファイル、同一ディレクトリ、シンボル一致、テスト/実装の対応関係
* lexicalはBM25/単純一致でも可（初期はrg出力+正規化で十分）
* semanticはTopKを粗探索→MMRで多様性確保（同じファイルばかりを避ける）

### 8.3 結果の提示

* Top N（デフォルト10）を以下で表示

  * `path:line_range`、短いスニペット、マッチ理由（lexical hit / semantic similarity / prior）
* 候補から

  * Enter：開く（CLI内ビューア or エディタ連携）
  * `a`：コンテキストに追加（次のCodexプロンプトに添付）

## 9. CLI/UI 仕様（スラッシュコマンド中心）

Codex CLIはスラッシュコマンドで操作でき、拡張ガイドもあるため、本Forkでも主要導線にする。([OpenAI Developers][5])

### 9.1 追加/拡張するコマンド（案）

* `/index`：索引作成（初回）

  * オプション：`--full`（全再作成）, `--dry-run`（対象数だけ表示）
* `/reindex`：索引の再作成（破損時/モデル変更時）
* `/search <query>`：ハイブリッド検索（デフォルト）

  * `--lex` / `--sem` で単独モード切替
* `/cache status`：キャッシュ統計（サイズ、最終更新、除外数）
* `/cache purge`：キャッシュ削除（リポ単位/全体）
* `/index watch on|off`：監視モード切替（省電力運用向け）

### 9.2 既存UIとの整合

* 既存の `/status` コマンド出力に「Index: Ready/Building」「Last update」等を追記
* 設定は `~/.codex/config.toml` を継承し、CLI側は `-c key=value` 上書き可能（既存仕様に準拠）([OpenAI Developers][2])

## 10. 設定仕様（`~/.codex/config.toml` 拡張案）

Codexの設定は `~/.codex/config.toml` に置かれ、CLI/拡張で共有される前提があるため、そこに追記できる形にする。([OpenAI Developers][1])

例（提案）

```toml
[index]
enabled = true
backend = "local"            # local | api
embedding_model = "text-embedding-3-large"
top_k = 30
mmr_lambda = 0.35
max_file_bytes = 2000000
chunk_target_bytes = 1800
watch = true
update_debounce_ms = 800

[cache]
enabled = true
dir = "~/.codex/cache"
tree_cache = true
chunk_cache = true
integrity_check_on_start = "light"  # off | light | full

[search]
mode = "hybrid"              # hybrid | lexical | semantic
hybrid_alpha = 0.65
hybrid_beta = 0.30
hybrid_gamma = 0.05

[ignore]
use_codexignore = true
use_gitignore = true
additional_globs = ["**/.env*", "**/secrets/**", "**/*.pem"]
```

## 11. 増分更新仕様（Change Detector）

### 11.1 監視方式

* OSネイティブ監視（inotify/FSEvents/ReadDirectoryChangesW）
* フォールバック：一定間隔の軽量スキャン（mtime比較）

### 11.2 イベント処理

* create/modify/delete/rename を受け取る
* 連続編集に備え、debounce（例：800ms）してまとめて処理
* renameは可能なら同一inode/近似hashで追跡し、索引の作り直しを最小化

### 11.3 エラー時

* 監視が落ちたら自動でフォールバックへ切替、`/status` に警告
* キャッシュ破損検出時は `integrity_check_on_start` に従い修復/再作成を提案

## 12. セキュリティ/安全性仕様

Codex CLIにはサンドボックスや承認フローがあるため、本Forkのインデクシング/検索がそれを迂回しないことを保証する。([OpenAI Developers][7])

要件

* `--sandbox read-only` 相当のモードでは、索引作成（書き込み）を禁止または「ユーザーキャッシュ領域のみ許可」に限定

  * 例：`~/.codex/cache` への書き込みは可、ワークスペース内の生成物書き込みは不可
* `.codexignore` 等の除外が有効な場合、索引にも検索にも絶対に載せない（漏れを防ぐ）([GitHub][4])
* `/cache purge` は確認プロンプト必須（誤操作防止）

## 13. パフォーマンス設計

* 初回索引：並列でチャンク生成＋embeddingバッチ（CPU/IOに応じて上限）
* 増分索引：小さいバッチで頻繁に回す（体感優先）
* メモリ上限：巨大リポでO(N)メモリにならない（ストリーミング処理）
* 低電力モード：watch off + 手動`/index`更新

## 14. テスト仕様

* ユニットテスト

  * ignoreマッチング、チャンク分割、hash安定性、融合スコア
* 統合テスト

  * 変更イベント→増分更新→検索結果に反映
  * `.codexignore` の尊重（索引・検索双方で完全除外）
* ベンチマーク

  * ファイル数1万/10万の疑似リポで起動・検索・更新時間を測定

## 15. リリース/運用

* マイグレーション：既存ユーザーは何もしなくても動作（索引は任意）
* 破壊的変更なし：設定未記載なら従来通り
* トラブルシュート：`/cache status` に診断情報（索引バージョン、破損検出、再作成手順）

---

必要なら、この仕様を「実装優先度（MVP→v1→v1.5）」に分解して、最初のPR単位（ファイル/モジュール構成、データスキーマ、コマンドUI）まで落とした実装計画にします。

[1]: https://developers.openai.com/codex/local-config/?utm_source=chatgpt.com "Configuring Codex"
[2]: https://developers.openai.com/codex/cli/reference/?utm_source=chatgpt.com "Codex CLI reference"
[3]: https://github.com/openai/codex/issues/5181?utm_source=chatgpt.com "openai/codex - Semantic codebase indexing and search"
[4]: https://github.com/openai/codex/issues/205?utm_source=chatgpt.com "[Feature Request] .codexignore file · Issue #205"
[5]: https://developers.openai.com/codex/guides/slash-commands/?utm_source=chatgpt.com "Slash commands in Codex CLI"
[6]: https://developers.openai.com/codex/guides/agents-md/?utm_source=chatgpt.com "Custom instructions with AGENTS.md"
[7]: https://developers.openai.com/codex/security/?utm_source=chatgpt.com "Codex security guide"

