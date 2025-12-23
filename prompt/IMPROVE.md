# Codex CLI 改善提案書

Claude Code リポジトリの分析に基づき、Codex CLI に追加すべき機能および改善点を洗い出した提案書。

---

## 概要

Claude Code（Anthropic製のエージェント型コーディングツール）の分析から、以下の観点で Codex CLI の強化ポイントを抽出：

1. **プラグインシステム** - 拡張性の大幅向上
2. **Hook システム** - イベント駆動の自動化
3. **エージェント機能** - 専門化されたサブエージェント
4. **コマンドシステム** - カスタムスラッシュコマンド
5. **開発ワークフロー統合** - Git/GitHub連携
6. **UX改善** - 操作性・視覚フィードバック
7. **パフォーマンス最適化** - 高速化・効率化

---

## 1. プラグインシステムの導入

### 1.1 概要

Claude Code は強力なプラグインシステムを持ち、以下を提供：
- カスタムコマンドの追加
- 専門エージェントの定義
- Hookによる自動化
- MCPサーバー連携

### 1.2 推奨する機能

#### プラグイン構造の標準化

```
plugin-name/
├── .codex-plugin/
│   └── plugin.json          # プラグインメタデータ
├── commands/                 # スラッシュコマンド（オプション）
├── agents/                   # 専門エージェント（オプション）
├── skills/                   # スキルファイル（オプション）
├── hooks/                    # イベントハンドラ（オプション）
└── README.md                 # プラグインドキュメント
```

#### plugin.json 仕様例

```json
{
  "$schema": "https://codex.openai.com/plugin.schema.json",
  "name": "code-review",
  "version": "1.0.0",
  "description": "Automated code review for pull requests",
  "author": "Your Name",
  "category": "productivity",
  "source": "./plugins/code-review"
}
```

### 1.3 プラグインマーケットプレイス

Claude Codeは公式プラグインマーケットプレイスを提供：
- `/plugin install <name>` でインストール
- `/plugin enable/disable` で有効化/無効化
- `/plugin marketplace` でブラウズ

**Codex CLIへの提案**：
- `~/.codex/plugins/` にプラグインを格納
- `config.toml` の `[plugins]` セクションで設定
- オフィシャルとコミュニティの両方のプラグインをサポート

---

## 2. Hook システム

### 2.1 概要

Claude Codeは豊富なHookシステムを持つ：

| Hook イベント | 説明 |
|--------------|------|
| `PreToolUse` | ツール実行前に発火（ブロック/警告可能） |
| `PostToolUse` | ツール実行後に発火 |
| `SessionStart` | セッション開始時に発火 |
| `SessionEnd` | セッション終了時に発火 |
| `Stop` | エージェント停止時に発火 |
| `SubagentStart` | サブエージェント起動時に発火 |
| `SubagentStop` | サブエージェント終了時に発火 |
| `PermissionRequest` | 権限リクエスト時に発火 |
| `UserPromptSubmit` | ユーザープロンプト送信時に発火 |
| `Notification` | 通知時に発火 |
| `PreCompact` | コンパクト処理前に発火 |

### 2.2 推奨する実装

#### hooks.json 構成例

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "python3 ~/.codex/hooks/bash_validator.py"
          }
        ]
      }
    ],
    "SessionEnd": [
      {
        "type": "command",
        "command": "~/.codex/hooks/cleanup.sh"
      }
    ]
  }
}
```

#### セキュリティHookの例（Claude Codeの security-guidance プラグインより）

```python
SECURITY_PATTERNS = [
    {
        "ruleName": "eval_injection",
        "substrings": ["eval("],
        "reminder": "⚠️ Security Warning: eval() executes arbitrary code..."
    },
    {
        "ruleName": "child_process_exec",
        "substrings": ["child_process.exec", "exec("],
        "reminder": "⚠️ Security Warning: Using child_process.exec() can lead to command injection..."
    },
    {
        "ruleName": "pickle_deserialization", 
        "substrings": ["pickle"],
        "reminder": "⚠️ Security Warning: Using pickle with untrusted content..."
    }
]
```

### 2.3 Hookify（ルール生成支援）

Claude Codeの `hookify` プラグインのような機能：
- `/hookify <説明>` で自然言語からHookルールを自動生成
- `/hookify:list` で現在のルール一覧
- Markdownベースの簡易設定

```markdown
---
name: block-dangerous-rm
enabled: true
event: bash
pattern: rm\s+-rf
action: block
---

⚠️ **Dangerous rm command detected!**
This command could delete important files.
```

---

## 3. 専門エージェント機能

### 3.1 Claude Code のエージェント一覧

Claude Codeは以下の専門エージェントを提供：

| エージェント | 用途 |
|-------------|------|
| `code-explorer` | コードベース探索、実行パストレース |
| `code-architect` | アーキテクチャ設計、実装計画 |
| `code-reviewer` | コードレビュー、バグ検出 |
| `code-simplifier` | コード簡素化、リファクタリング |
| `comment-analyzer` | コメント精度分析 |
| `pr-test-analyzer` | テストカバレッジ分析 |
| `silent-failure-hunter` | エラーハンドリング不備検出 |
| `type-design-analyzer` | 型設計品質分析 |
| `conversation-analyzer` | 会話パターン分析（Hookify用） |
| `plugin-validator` | プラグイン検証 |

### 3.2 推奨するCodex CLIへのエージェント追加

#### A. Index Explorer Agent

```yaml
name: index-explorer
description: セマンティック索引を活用したコードベース探索
triggers:
  - "このコードベースの概要を教えて"
  - "〇〇に関連するコードを探して"
focus:
  - ハイブリッド検索の活用
  - ファイル間の関連性分析
  - 依存関係の可視化
```

#### B. Performance Analyzer Agent

```yaml
name: performance-analyzer
description: パフォーマンスボトルネックの検出
triggers:
  - "パフォーマンス問題を調べて"
  - "遅い処理を見つけて"
focus:
  - N+1クエリ検出
  - メモリリーク検出
  - 非効率なアルゴリズム検出
```

#### C. Security Auditor Agent

```yaml
name: security-auditor
description: セキュリティ脆弱性の検出
triggers:
  - "セキュリティチェック"
  - "脆弱性を探して"
focus:
  - SQLインジェクション
  - XSS
  - コマンドインジェクション
  - 秘密情報の露出
```

### 3.3 エージェント定義形式

```markdown
---
name: code-reviewer
model: gpt-4o
description: コードレビュー専門エージェント
triggers:
  - "レビューして"
  - "コードをチェック"
allowedTools:
  - Read
  - Search
  - Bash(grep:*)
  - Bash(git diff:*)
---

# コードレビューエージェント

あなたはコードレビューの専門家です。以下の観点でレビューを行います：

1. **バグ検出**: 明らかなバグやエラー処理の不備
2. **規約準拠**: プロジェクトの AGENTS.md に従っているか
3. **コード品質**: 可読性、保守性、DRY原則
4. **セキュリティ**: 潜在的な脆弱性

## 出力形式

問題点を信頼度スコア(0-100)と共に報告してください。
```

---

## 4. カスタムスラッシュコマンド強化

### 4.1 Claude Code のコマンドシステム

Claude Codeはマークダウンベースのカスタムコマンドをサポート：

```markdown
---
allowed-tools: Bash(git:*), Bash(gh:*)
description: Commit, push, and open a PR
---

## Context
- Current git status: !`git status`
- Current git diff: !`git diff HEAD`
- Current branch: !`git branch --show-current`

## Your task
Based on the above changes:
1. Create a new branch if on main
2. Create a single commit with an appropriate message
3. Push the branch to origin
4. Create a pull request using `gh pr create`
```

### 4.2 推奨するCodex CLI コマンド追加

#### `/commit-push-pr`
Git操作の自動化：変更をコミット→プッシュ→PR作成まで一括実行

#### `/code-review`
複数の専門エージェントを並列起動してPRをレビュー

#### `/feature-dev`
7フェーズの構造化された機能開発ワークフロー：
1. Discovery（要件理解）
2. Codebase Exploration（既存コード分析）
3. Clarifying Questions（曖昧さの解消）
4. Architecture Design（設計選択肢の提示）
5. Implementation（実装）
6. Quality Review（品質レビュー）
7. Summary（完了報告）

#### `/index-status`
索引の状態表示（SPEC.mdの `/cache status` を拡張）

#### `/search` 強化
```
/search "認証トークンの更新" --mode=hybrid --top=20
/search "AuthService" --mode=lexical
/search "ユーザー認証の流れ" --mode=semantic
```

---

## 5. 開発ワークフロー統合

### 5.1 GitHub 連携強化

Claude Codeの機能を参考に：

#### PR Review 自動コメント
```
/code-review --post-comment
```
- 複数エージェントで並列レビュー
- 信頼度スコア80以上の問題のみコメント
- AGENTS.md 準拠チェック

#### Duplicate Issue 検出
```
/dedupe <issue-number>
```
- 類似Issue を自動検出
- 重複候補をコメント投稿

#### Git Workflow コマンド
```
/commit          # インテリジェントなコミットメッセージ生成
/commit-push-pr  # 一括処理
/clean-gone      # マージ済みブランチの削除
```

### 5.2 IDE 連携

Claude Code は VSCode 拡張を提供：
- リアルタイムストリーミング
- ドラッグ&ドロップでファイル追加
- インラインdiff表示

**Codex CLIへの提案**：
- LSP連携によるシンボル解決
- エディタからの直接起動
- diff結果のエディタ表示

---

## 6. UX 改善

### 6.1 操作性の向上

| 機能 | 説明 |
|------|------|
| `@` メンション | ファイル・ディレクトリをタイプアヘッドで参照 |
| Tab補完 | ファイル名・コマンドの自動補完 |
| Ctrl+R | 履歴検索 |
| Shift+Tab | モード切替（auto-accept等） |
| Ctrl+G | システムエディタでプロンプト編集 |
| Option+P | モデル切替 |

### 6.2 セッション管理

```
/resume            # 前回のセッションを再開
/resume <name>     # 名前付きセッションを再開
/rename <name>     # 現在のセッションに名前付け
/rewind            # 会話を巻き戻してコード変更を取り消し
```

### 6.3 視覚フィードバック

- プログレスバー（索引作成、ファイル処理）
- スピナーアニメーション（思考中表示）
- 色分けされた出力（警告:黄、エラー:赤、成功:緑）
- 統計情報表示（`/stats`、`/usage`）

### 6.4 国際化対応

Claude Code は IME（Input Method Editor）対応を強化：
- CJK文字のカーソル移動
- 変換ウィンドウの正しい位置表示
- 単語境界の適切な判定

---

## 7. パフォーマンス最適化

### 7.1 Claude Code の最適化事例

| 項目 | 施策 |
|------|------|
| メモリ使用量 | 大規模会話で3倍改善 |
| 起動時間 | ネイティブバイナリ化で高速化 |
| auto-compact | 即時実行化 |
| ファイル検索 | Rust製ファジーファインダー導入 |
| 設定変更 | 再起動不要で即時反映 |

### 7.2 Codex CLI への適用

#### A. 増分索引の最適化
- ファイル変更検知のdebounce（800ms推奨）
- 変更チャンクのみ再embedding
- バックグラウンド非同期更新

#### B. 検索の高速化
- 粗探索（TopK）→詳細探索（MMR）の2段階
- lexical検索とsemantic検索の並列実行
- キャッシュヒット時の即座返却

#### C. メモリ効率
- ストリーミング処理（O(N)メモリ回避）
- 大きなファイルの分割読み込み
- 未使用キャッシュの自動解放

---

## 8. セキュリティ機能

### 8.1 セキュリティリマインダーHook

Claude Codeの `security-guidance` プラグインを参考に：

```python
# 監視対象パターン
SECURITY_PATTERNS = [
    "child_process.exec",    # コマンドインジェクション
    "eval(",                 # コード実行
    "new Function",          # 動的コード生成
    "dangerouslySetInnerHTML", # XSS
    "document.write",        # XSS
    ".innerHTML =",          # XSS
    "pickle",                # デシリアライズ攻撃
    "os.system",             # シェルインジェクション
]
```

### 8.2 GitHub Actions セキュリティ

`.github/workflows/` ファイル編集時の警告：

```
⚠️ GitHub Actions workflow file editing detected!

1. **Command Injection**: Never use untrusted input directly in run: commands
2. **Use environment variables**: Instead of ${{ github.event.issue.title }}, 
   use env: with proper quoting
   
Risky inputs to watch:
- github.event.issue.body
- github.event.pull_request.title
- github.event.comment.body
- github.head_ref
```

### 8.3 サンドボックスモード

```toml
[security]
sandbox_mode = "read-only"  # off | read-only | strict
require_approval_for = [
  "Bash(rm:*)",
  "Bash(chmod:*)",
  "Write(*.env*)",
  "Write(*secret*)"
]
```

---

## 9. 設定拡張案（config.toml）

### 9.1 プラグイン設定

```toml
[plugins]
enabled = true
marketplace_url = "https://codex.openai.com/plugins"
auto_update = true
installed = [
  "code-review",
  "security-guidance",
  "commit-commands"
]

[plugins.code-review]
confidence_threshold = 80
post_comments = true
```

### 9.2 エージェント設定

```toml
[agents]
enabled = true
default_model = "gpt-4o"
max_parallel = 3
timeout_seconds = 300

# カスタムエージェントへのパス
custom_agents_dir = "~/.codex/agents"
project_agents_dir = ".codex/agents"
```

### 9.3 Hook設定

```toml
[hooks]
enabled = true
config_file = "~/.codex/hooks.json"
timeout_ms = 30000

# 特定Hookの無効化
disabled_hooks = ["PostToolUse"]
```

### 9.4 UI設定

```toml
[ui]
spinner_tips_enabled = true
progress_bar = true
color_theme = "auto"  # auto | light | dark | ansi
vim_mode = false
verbose = false
```

---

## 10. 実装優先度

### Phase 1: MVP（最優先）

| 機能 | 理由 |
|------|------|
| 基本Hook（PreToolUse, PostToolUse） | セキュリティ・自動化の基盤 |
| カスタムコマンド（Markdown形式） | 拡張性の基盤 |
| `/search` 強化（ハイブリッド対応） | 既存SPEC.mdとの整合 |
| セッション管理（/resume, /rename） | UX向上 |

### Phase 2: v1.0

| 機能 | 理由 |
|------|------|
| プラグインシステム基盤 | 拡張エコシステムの構築 |
| 専門エージェント（2-3種） | 複雑タスクの効率化 |
| GitHub連携コマンド | 開発ワークフロー統合 |
| セキュリティHook | 安全性向上 |

### Phase 3: v1.5

| 機能 | 理由 |
|------|------|
| プラグインマーケットプレイス | コミュニティ拡大 |
| IDE連携（VSCode拡張） | 開発体験向上 |
| 全Hookイベント実装 | 完全な自動化対応 |
| パフォーマンス最適化 | 大規模リポ対応 |

---

## 11. 参考資料

### Claude Code リポジトリ構成

```
claude-code/
├── .claude/
│   └── commands/           # 内蔵コマンド
├── .claude-plugin/
│   └── marketplace.json    # プラグインカタログ
├── plugins/                # 公式プラグイン（13種）
│   ├── agent-sdk-dev/
│   ├── code-review/
│   ├── commit-commands/
│   ├── feature-dev/
│   ├── frontend-design/
│   ├── hookify/
│   ├── learning-output-style/
│   ├── plugin-dev/
│   ├── pr-review-toolkit/
│   ├── ralph-wiggum/
│   └── security-guidance/
├── examples/
│   └── hooks/              # Hookサンプル
└── CHANGELOG.md            # 1129行の機能履歴（v0.2.21〜v2.0.70）
```

### 主要な機能リリース履歴（抜粋）

| バージョン | 機能 |
|-----------|------|
| v2.0.0 | VSCode拡張、/rewind、/usage |
| v2.0.12 | プラグインシステム |
| v2.0.20 | Claude Skills |
| v2.0.28 | Plan Mode（計画サブエージェント） |
| v2.0.45 | PermissionRequest Hook |
| v2.0.60 | カスタムサブエージェント |
| v2.0.64 | エージェントモデルカスタマイズ |
| v2.0.70 | MCP権限ワイルドカード |

---

## 結論

Claude Code の分析から、Codex CLI を大幅に強化できる機能が多数存在する。特に以下の3点を優先的に実装することを推奨：

1. **Hook システム** - セキュリティと自動化の基盤として必須
2. **プラグインシステム** - 拡張エコシステム構築の鍵
3. **専門エージェント** - 複雑タスクの効率化に直結

これらの機能は既存の SPEC.md（増分キャッシュ＋セマンティック索引）と自然に統合でき、Codex CLI の価値を大きく向上させる。
