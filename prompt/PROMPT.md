# codex-indexer 用タスク実行プロンプト

`prompt/PLAN.md`（進行中PR）と `prompt/SPEC.md`（仕様）を必ず確認してから作業を開始する。対象PRの例: PR-2（grep cache）など。

## 実装フロー

### 1. ブランチ作成
- `main` から `pr-<番号>-<簡潔な説明>` で切る（例: `pr-2-grep-cache`）

### 2. 準備・方針
- PLANで決まっているファイル/モジュール/コマンドUIを外さない。必要なら `docs/` や `prompt/` の補足も追加。
- 可能な範囲で先に失敗するテストを書き、最小実装→リファクタの順で進める。
- Rust変更は `codex-rs/` 配下。clippy系の掟（format!の引数インライン化、ifの折り畳み、不要なクロージャ回避等）と `CODEX_SANDBOX_*` を触らない方針を守る。

### 3. フォーマット・Lint・テスト（codex-rs）
```bash
cd codex-rs
just fmt                                 # Rustを触ったら必ず
just fix -p codex-<project>              # 触ったクレート単位で実施（共有クレートならworkspace）
cargo test -p codex-<project>            # 変更箇所のテスト
cargo test --all-features                # common/core/protocol を変えたら（重いので必要性を確認）
# 広く確認したい場合は just test (= cargo nextest run --no-fail-fast)
```

### 4. PLAN/ドキュメント更新
- `prompt/PLAN.md` の該当PRを `[DONE]` にし、`Current:` と `Tests:` に実績を追記。
- API/CLIが変わる場合は `docs/` も同期。

### 5. コミット & PR
- メッセージ形式: `<type>(<scope>): <description>` （例: `feat(core): add grep cache ttl`）
- 適切な粒度で分割し、`main` に対する PR を作成（タイトルに PR 番号を含める）。

## チェックリスト

- [ ] 対象PRを `prompt/PLAN.md` / `prompt/SPEC.md` で確認した
- [ ] ブランチを `pr-<番号>-...` で `main` から切った
- [ ] 失敗するテストを用意してから実装した
- [ ] `just fmt` / `just fix -p ...` を実行した
- [ ] 必要なテストを通した（+共有クレート変更時は `cargo test --all-features`）
- [ ] `prompt/PLAN.md` と関連ドキュメントを更新した
- [ ] コミットメッセージが規約に沿っている
