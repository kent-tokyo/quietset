# quietset

[English](README.md) | **日本語**

[![CI](https://github.com/kent-tokyo/quietset/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/quietset/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/quietset.svg)](https://crates.io/crates/quietset)
[![docs.rs](https://docs.rs/quietset/badge.svg)](https://docs.rs/quietset)

モデルに依存しない安定性フィルタ — 評価者・budget・seed・モデルチェックポイントをまたいでラベルやスコアが一致するサンプルだけを残します。

quietset はモデルの訓練ツールでも、アノテーションプラットフォームでも、画像品質チェッカーでもありません。他のツールと組み合わせて使う、小さな安定性フィルタリングのプリミティブです。

> **注意:** quietset が測るのは「安定性」であり「正確性」ではありません。評価者が一致して間違えるサンプルは高スコアになります。`gold_label` ベースの reliability や `--decision-score lcb` を使って証拠量ベースの保守性を加えてください。

## ユースケース

### ゲーム AI / 探索学習データ

複数エンジン・探索深さ・seed で同じ局面を評価し、評価値やラベルが安定している局面だけを学習データに残す。`--profile game-ai` は評価値の符号一致も重み付けし、`--decision-score` は自動的に `lcb` になるため、ゼロ付近の小さな評価値が探索ノイズで符号反転しているのに「安定」と誤判定されることを防ぐ（生の stability_score だけでは keep/drop の保証にならない理由は後述の `stability_score` を参照）。複数の異なるエンジンで評価する場合は `--profile game-ai` を、1エンジンを複数の探索深さで評価する場合は `--profile game-ai-single-engine` を使う — 重みは同じだが、1エンジン構成では満たせない最低evaluator数の下限がない。

```bash
quietset score positions.jsonl --profile game-ai > scored.jsonl
quietset stable-wrong-risk positions.jsonl  # 安定して誤ラベルの局面を検出

# 全局面を再評価する代わりに、追加探索する価値がある局面だけに budget を割り当てる。
quietset active-review scored.jsonl --observations positions.jsonl \
  --plan next_eval_plan.jsonl --budget 2000

# 学習前に、held-out データでの精度が基準を満たすかをゲートする。
quietset calibrate train_gold.jsonl --heldout heldout_gold.jsonl \
  --target-precision 0.98 --fail-below-target
```

### LLM judge パイプライン

複数の judge モデルやプロンプトで同じ回答を評価し、一致率の高い回答だけを残す。Wilson LCB で少数観測の過信を防ぐ。

```bash
quietset score judge_evals.jsonl --profile llm-judge > reliable_evals.jsonl
quietset calibrate judge_evals.jsonl --target-precision 0.95 --decision-score lcb
```

### 合成データ / シミュレーション

seed・budget・モデルチェックポイントをまたいでスコアが安定しているサンプルだけを残す。

```bash
quietset score runs.jsonl --profile simulation > robust_samples.jsonl
quietset audit robust_samples.jsonl --json | jq '.seed_sensitive[:5]'
```

## インストール

```bash
cargo install quietset-cli
```

## CLI の使い方

```bash
# 観測データをスコアリング
quietset score input.jsonl > scored.jsonl

# 安定したサンプルだけに絞り込む
quietset filter scored.jsonl --min-stability 0.85 > quiet.jsonl

# 判定結果で絞り込む
quietset filter scored.jsonl --decision keep > keep.jsonl

# stdin からパイプ
cat runs/*.jsonl | quietset score - > scored.jsonl

# データセット全体の集計統計を表示
quietset summary scored.jsonl

# CI 用 JSON 出力（jq との連携）
quietset summary scored.jsonl --json | jq '.drop_rate < 0.1'

# 特定サンプルのスコア内訳を表示
quietset explain scored.jsonl --sample-id a

# 2つの scored ファイルを比較（モデル更新前後など）
quietset compare before.jsonl after.jsonl

# 評価者ごとの信頼度を推定（experimental）
quietset reliability input.jsonl

# CSV 出力（pandas・R との連携に便利）
quietset score input.jsonl --output-format csv > scored.csv

# ラベル合意を2倍重視、スコア分散を無視
quietset score input.jsonl --weight-labels 2.0 --weight-scores 0.0 > scored.jsonl

# 証拠の薄いサンプルにペナルティ: confidence 調整スコアで判定
quietset score input.jsonl --use-adjusted-score > scored.jsonl

# 証拠の薄いサンプルにペナルティ: Wilson LCB（最も保守的）
quietset score input.jsonl --use-lcb-score > scored.jsonl

# LCB の信頼水準を調整（デフォルト 0.95）
quietset score input.jsonl --use-lcb-score --confidence-level 0.99 > scored.jsonl

# 明示的な --decision-score フラグ（スクリプトでの利用に推奨。--use-* は alias）
quietset score input.jsonl --decision-score lcb > scored.jsonl
quietset score input.jsonl --decision-score adjusted > scored.jsonl

# プロファイルプリセット（weight と decision-score のデフォルトを設定）
quietset score input.jsonl --profile llm-judge > scored.jsonl
quietset score input.jsonl --profile simulation > scored.jsonl

# 最低3件の観測・2人の評価者がなければ keep にしない
quietset score input.jsonl --min-observations-keep 3 --min-evaluators-keep 2 > scored.jsonl

# LCB・confidence・スコア分散でフィルタ
quietset filter scored.jsonl --min-label-lcb 0.70 > filtered.jsonl
quietset filter scored.jsonl --min-confidence 0.60 --max-score-mad 0.05 > filtered.jsonl

# コンポーネント差分つきで比較（劣化箇所を特定）
quietset compare before.jsonl after.jsonl --components

# 深掘り診断レポート
quietset audit scored.jsonl
quietset audit scored.jsonl --json | jq '.high_raw_low_lcb'

# gold_label から keep 閾値を自動探索
quietset calibrate input.jsonl --target-precision 0.95
quietset calibrate input.jsonl --target-precision 0.98 --decision-score lcb
```

## コマンドリファレンス

| コマンド | 入力 | 用途 |
|---------|------|------|
| `score` | 観測 JSONL/CSV | 安定性スコアと判定を計算 |
| `filter` | scored JSONL | 安定性・判定・LCB・confidence・分散で絞り込み |
| `summary` | scored JSONL | 集計統計、`lcb_keep_demotions`、`--json` で CI 連携 |
| `explain` | scored JSONL | サンプル単位のコンポーネント内訳（ビジュアルバー付き） |
| `compare` | scored JSONL × 2 | 前後比較、コンポーネント差分、ポリシー比較 |
| `reliability` | 観測 JSONL | 評価者信頼度、混同行列、Fleiss kappa、Krippendorff alpha |
| `audit` | scored JSONL | 深掘り診断レポート（borderline / LCB リスク / 感度リスト） |
| `select` | scored JSONL | 診断クラスでサンプル抽出（パイプ対応） |
| `recommend` | scored JSONL | 再評価候補と理由を提案 |
| `stable-wrong-risk` | 観測 JSONL | 安定的に誤ラベルの keep サンプル率（`gold_label` 必須） |
| `calibrate` | 観測 JSONL | 精度目標を満たす keep 閾値を自動探索 |
| `policy` | 観測 JSONL | keep_threshold を掃引し precision/coverage のトレードオフ表を表示 |
| `active-review` | scored JSONL | 再評価の緊急度（低LCB・高エントロピー・分散・感度）でサンプルを順位付け |
| `block-score` | 観測 JSONL | `block_id` でグループ化し、ブロック単位の軌道安定性を分類 |
| `trajectory-audit` | 観測 JSONL × 2 | before/after checkpoint 間のブロック単位の軌道安定性を比較 |

## 出力フォーマット

出力の慣習はコマンドごとに意図的に異なる。人間による確認を主目的とするコマンドと、パイプライン合成を主目的とするコマンド（[`select`](#select-コマンド)や[`--embed-stats`](#audit-コマンド)を参照）が混在しているためで、単一の統一フォーマットは存在しない。スクリプトを書く前に以下の表で各コマンドの出力形状を確認すること:

| コマンド | デフォルト | フラグ | 備考 |
|---------|-----------|--------|------|
| `score` | JSONL | `--output-format jsonl\|csv` | `csv`は終端フォーマット（下記参照） |
| `filter` | JSONL（パススルー） | なし | 元の入力行をそのまま出力 |
| `select` | JSONL（パススルー） | なし | 元の入力行をそのまま出力 |
| `reliability` | JSONL | なし | 評価者ごとに1オブジェクト＋末尾にkappa/alphaの行（任意） |
| `active-review` | JSONL | なし | 順位付けされたサンプルごとに1オブジェクト |
| `block-score` | JSONL | なし | ブロックごとに1オブジェクト |
| `trajectory-audit` | text | `--json`（JSONL、ブロックごとに1オブジェクト） | 最も破壊的なブロックから順に表示 |
| `recommend` | JSONL | `--text` | JSONLがデフォルトでtextが例外という唯一のコマンド |
| `stable-wrong-risk` | 単一のpretty JSONオブジェクト | なし | |
| `calibrate` | 単一のpretty JSONオブジェクト | `--output-format json\|csv` | `csv`はヘッダー行1つ＋データ行1つ |
| `summary` | text | `--json`（単一のpretty JSONオブジェクト） | |
| `explain` | text | `--json`（単一のpretty JSONオブジェクト） | |
| `compare` | text | `--json`（単一のpretty JSONオブジェクト） | |
| `audit` | text | `--json`（単一のpretty JSONオブジェクト） | |
| `policy` | textテーブル | `--output-format text\|json\|csv`（旧`--json`は`json`のエイリアスとして引き続き使用可） | `json`はJSONL(閾値ごとに1行)、`csv`は固定列(`gold_label`が無い場合は空セル) | |

**CSVはパイプ用途では行き止まり。** `score --output-format csv`、`calibrate --output-format csv`、`policy --output-format csv`はスプレッドシート/BIツール向けのエクスポート専用。他のどのquietsetコマンドもCSVを読み戻せない（`filter`/`summary`/`explain`/`compare`/`audit`/`select`/`recommend`/`active-review`はいずれもJSONL形式の`StabilityReport`を期待し、`stable-wrong-risk`/`calibrate`/`reliability`/`policy`はJSONL形式の`Observation`を期待する）。`score`の出力を他のquietsetコマンドにパイプする予定があるなら、`csv`ではなくデフォルトの`jsonl`を使うこと。

## 入力 JSONL フォーマット

`sample_id` 以外のフィールドはすべて省略可能です。あるフィールドだけを含むサブスコアのみが安定性計算に使われます。

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1,"gold_label":"win"}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1,"gold_label":"win"}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

`gold_label` はサンプルの既知の正解ラベルです。指定されている場合、`reliability` コマンドが多数派ラベルの代わりにこれを正解として使います。

### 軌道安定性フィールド（省略可能）

学習ハーネス側で事前計算した実行ごとの軌道信号を、`block-score`/`trajectory-audit`および下記の`score`コンポーネント向けに渡すための、追加の省略可能フィールドが10個あります。quietsetはこれらの数値を集計するだけで、checkpointを読んだり勾配を自分で計算したりすることは一切ありません:

```json
{"sample_id":"p1","block_id":"B5","seed":1,"shuffle_seed":1,"model_id":"ckpt143","loss_recipe":"recipe_a","layer_id":"ft","gradient_sign":0.3,"update_cosine":0.8,"teacher_residual":0.02,"trajectory_effect":-0.4,"dead_unit_count":0,"saturated_unit_count":0}
```

| フィールド | 意味 |
|-----------|------|
| `shuffle_seed` | 第2のシード軸（データ順序/シャッフル）。`seed`（初期化シード）とは別軸 |
| `loss_recipe` | 損失関数/学習レシピのバリアント — `model_id`/`evaluator_id`と同じ役割の条件軸 |
| `block_id` | サンプルを学習ブロック（例: 32局面ブロック）にグループ化する。`sample_id`とは別 |
| `layer_id` | `dead_unit_count`/`saturated_unit_count`用の汎用レイヤー識別子 — quietsetは値そのものに意味を持たせない（例: `"ft"`、`"l2"`は単なる文字列） |
| `gradient_sign` | 符号付きの実行ごとのスカラー値。符号のみが使われる |
| `update_cosine` | この更新方向と基準方向とのコサイン類似度（実行ごと） |
| `teacher_residual` | 符号付きの実行ごとのスカラー値（例: モデル出力からteacher出力を引いた値） |
| `trajectory_effect` | ブロックの成長/縮小分類を駆動する符号付きの実行ごとのスカラー値（例: radial normの変化量）。正=成長、負=縮小 |
| `dead_unit_count` | この（sample, run）における`layer_id`内の新規dead unit数 |
| `saturated_unit_count` | この（sample, run）における`layer_id`内の新規saturated unit数 |

## 出力 JSONL フォーマット

主要フィールド（計算できない場合は省略されます）:

```json
{
  "sample_id": "a",
  "n_observations": 2,
  "majority_label": "win",
  "label_agreement": 1.0,
  "label_agreement_lcb": 0.342,
  "label_margin": 1.0,
  "label_entropy": 0.0,
  "score_mean": 0.895,
  "score_std": 0.015,
  "score_mad": 0.015,
  "score_iqr": 0.030,
  "confidence": 0.40,
  "adjusted_stability_score": 0.782,
  "stability_score": 0.97,
  "decision": "keep",
  "components": {
    "label": 1.0,
    "score_consistency": 0.985
  }
}
```

省略可能なフィールドは計算できない場合は出力されません（例: `label_agreement_lcb` はラベルがある場合のみ、`score_mad` / `score_iqr` はスコアが2件以上ある場合のみ）。

## 安定性スコア（stability_score）

`stability_score` は `[0.0, 1.0]` の値です。

- `1.0` = 非常に安定
- `0.0` = 非常に不安定

`stability_score` は**ランキング・確認用の convenience aggregate であり、キャリブレーションされた確率でも正しさの保証でもありません**。それ自体に不確実性モデルを持たないため、全評価者が一貫して間違った答えに合意しているサンプルでも `1.0` になります。実際の accept/reject の判断には、生の `stability_score`/`--keep-threshold` のデフォルトではなく、統計的・モデルベースの根拠を持つ指標 — `label_agreement_lcb`（Wilson interval）、`--decision-score latent-truth`（Dawid-Skene EM）、`gold-weighted`、または `calibrate --heldout` で選んだ閾値 — を優先してください。以下の各フィールドの完全な出典については [これらの数値の根拠](#これらの数値の根拠) と [docs/metrics_ja.md](docs/metrics_ja.md) を参照してください。

利用可能なサブスコアの**加重平均**として計算されます（すべて `[0.0, 1.0]`）。

| コンポーネント | 意味 |
|--------------|------|
| `label_agreement` | 多数派ラベルを持つ観測の割合 |
| `score_consistency` | `1 - 正規化スコア標準偏差` |
| `budget_robustness` | `1 - budget_sensitivity`（計算コスト変化への頑健性） |
| `seed_robustness` | `1 - seed_sensitivity`（シード変化への頑健性） |
| `model_agreement` | モデル間のラベル一致度 |
| `evaluator_agreement` | 評価者間のラベル一致度 |
| `score_sign_agreement` | 数値スコアの多数派符号が占める割合 — デフォルトの重みは `0`（除外）。`--weight-score-sign` で有効化できる（`--profile game-ai` はデフォルトで `×2` 有効） |
| `gradient_sign_agreement` | `gradient_sign` の多数派符号が占める割合 — デフォルトの重みは `0`。`--weight-gradient-sign` で有効化 |
| `update_direction_agreement` | `update_cosine` の多数派符号が占める割合 — デフォルトの重みは `0`。`--weight-update-cosine` で有効化 |
| `teacher_residual_stability` | `1 - normalized_std(teacher_residual)` — デフォルトの重みは `0`。`--weight-teacher-residual` で有効化 |
| shuffle-seed robustness | `1 - shuffle_seed_sensitivity` — デフォルトの重みは `0`。`--weight-shuffle-seed` で有効化 |
| `loss_recipe_agreement` | `loss_recipe` 間のラベル一致度 — デフォルトの重みは `0`。`--weight-loss-recipe` で有効化 |

対応するフィールドがない次元（例：ラベルなし、budget なし）は平均から除外されます。観測が1件のみのサンプルは `stability_score = 0.5`（デフォルトで `review`）になります。

追加の診断フィールド:

| フィールド | 意味 |
|----------|------|
| `label_margin` | `(多数派数 - 2位数) / 合計`。0.0 = 完全に拮抗 |
| `label_entropy` | 正規化シャノンエントロピー [0, 1]。1.0 = ラベルが均等分布 |
| `label_agreement_lcb` | `label_agreement` の Wilson 信頼区間下限。少数観測の過信を防ぐ保守的な指標。 |
| `score_mad` | スコアの中央絶対偏差（MAD）。外れ値への耐性が `score_std` より高い。 |
| `score_iqr` | スコアの四分位範囲（Q3 − Q1）。 |
| `score_sign_agreement` | 数値スコアの多数派符号（負・0・正）が占める割合。符号付きでゼロ中心のスコア（将棋・チェス系の評価値）向けの、magnitude 系指標（`score_std`/`score_mad`/`score_iqr`）を補完する指標 — `[+0.01, -0.01]` は magnitude だけ見るとほぼ完全に安定に見えるが、符号は五分五分。常に計算されるが、デフォルトでは `stability_score` から除外される（`--weight-score-sign 0`）。`--weight-score-sign <f>` を渡すと寄与するようになる。`--profile game-ai` はデフォルトで `×2` 有効にする — 他のすべてのプロファイルでは、明示的に重み付けしない限り情報提供のみのフィールドのまま。 |
| `budget_slope` | budget 増加に対するスコアの傾き（正 = 上昇収束） |
| `confidence` | `n / (n + k)` — 証拠量に基づくスコアの信頼度 |
| `adjusted_stability_score` | `stability * confidence + 0.5 * (1 - confidence)` |
| `update_cosine_mean` | `update_cosine` の平均値。診断のみで `stability_score` のコンポーネントではない |
| `dead_unit_rate` / `saturated_unit_rate` | `dead_unit_count`/`saturated_unit_count`が`0`より大きい観測の割合 — このサンプルが新規dead/saturatedユニットをどれだけ誘発するか。診断のみで`stability_score`には含まれず、`block-score`のpathological分類に使われる |
| `trajectory_effect_mean` / `trajectory_effect_sign_agreement` | `trajectory_effect`の平均値と符号一致率。診断のみで`stability_score`には含まれず、`block-score`の成長/縮小分類に使われる |

### components フィールドで理由を確認できる

各サブスコアは `components` に含まれます。

```json
{
  "sample_id": "a",
  "stability_score": 0.91,
  "decision": "keep",
  "components": {
    "label": 1.0,
    "score_consistency": 0.96,
    "budget_robustness": 0.88
  }
}
```

`--weight-*` フラグで個別調整するか、`--profile` を使ってください（→ [プロファイル](#プロファイル)）。

## プロファイル

手動で重みを調整する代わりに、`--profile` でユースケースプリセットを適用できます。明示的な `--weight-*` / `--decision-score` はプリセットより優先されます。

| プロファイル | 重み変更 | デフォルト decision-score |
|------------|--------|------------------------|
| `llm-judge` | evaluator ×2、model ×2 | `lcb` |
| `simulation` | budget ×2、seed ×2 | `adjusted` |
| `game-ai` | budget ×2、seed ×2、score_sign ×2、最低観測数 4、最低budget数 2、最低seed数 2、最低evaluator数 2 | `lcb` |
| `game-ai-single-engine` | `game-ai` と同じ重みだが最低evaluator数の下限なし — 1エンジンを複数深さで評価する場合向け | `lcb` |
| `benchmark` | label ×2、evaluator ×1.5 | `raw` |

```bash
# LLM judge プリセット（--weight-evaluators 2 --weight-models 2 --decision-score lcb と等価）
quietset score input.jsonl --profile llm-judge > scored.jsonl

# プリセットから一部を上書き
quietset score input.jsonl --profile simulation --weight-budget 3.0 > scored.jsonl
```

## confidence と adjusted_stability_score

`confidence = n / (n + k)`（k のデフォルトは 3.0）。

| 観測数 | confidence (k=3) |
|-------|-----------------|
| 1 | 0.25 |
| 2 | 0.40 |
| 5 | 0.63 |
| 10 | 0.77 |
| 20 | 0.87 |

`adjusted_stability_score = stability_score × confidence + 0.5 × (1 - confidence)`

例: `stability_score = 0.95` でも観測2件なら `adjusted_stability_score ≈ 0.68` — keep 閾値（0.85）に達しにくくなります。

`--use-adjusted-score` フラグで adjusted score ベースの判定に切り替えられます。`--confidence-k` で収束速度を調整できます。

## keep 判定の最低観測条件

高い安定性スコアだけでは不十分な場合があります。証拠が薄いサンプルを review に落とすには `--min-*-keep` を使います。

```bash
quietset score input.jsonl \
  --min-observations-keep 3 \
  --min-evaluators-keep 2 \
  --min-seeds-keep 2 \
  > scored.jsonl
```

## 判定（decision）

デフォルトでは `stability_score` が判定に使われます。5つのモードがあります。

| フラグ | alias | 使用スコア | 特性 |
|-------|-------|----------|------|
| `--decision-score raw` *（デフォルト）* | — | `stability_score` | 生の安定性。速いが少数観測の過信リスクあり。 |
| `--decision-score adjusted` | `--use-adjusted-score` | `adjusted_stability_score` | 証拠量に比例してペナルティ。 |
| `--decision-score lcb` | `--use-lcb-score` | `label_agreement_lcb`（ラベル部分） | Wilson LCB — 最も保守的。2/2 一致でも LCB ≈ 0.34（95%信頼）になり、証拠なしでは keep にならない。 |
| `--decision-score gold-weighted` | — | `stability_score`（ラベル部分を信頼性で重み付け） | 生の多数決の代わりに `gold_label` の一致履歴に基づく信頼性重み付き投票を使用。バッチ全体に `gold_label` が一つも無い場合は `raw` と同一の挙動になる。 |
| `--decision-score latent-truth` | — | `stability_score`（ラベル部分をEMで重み付け） | 生の多数決の代わりに Dawid-Skene EM が推定した投票を使用 — `gold_label` なしで、不一致パターンだけから評価者ごとの信頼性を推定する。バッチ全体で評価者またはラベルの種類が2未満の場合は `raw` と同一の挙動になる。出力に `latent_truth_label`、`latent_truth_confidence`、`latent_truth_label_distribution`、`majority_latent_conflict`、`evaluator_effective_n`、`correlated_evaluator_warning`、`latent_truth_converged`、`latent_truth_iterations`、`latent_truth_convergence_delta`、`latent_truth_demotion_reason` が追加される。`latent_truth_confidence` はEMが当てはめた事後確率であり、評価者が少ないと 0/1 に張り付きやすく、評価者のバイアスが相関している場合は自信満々に間違うこともある — 正しさの保証ではない。`evaluator_effective_n`/`correlated_evaluator_warning` は同じリスク（相関した評価者誤りに対する Kish design effect）を診断するが、*他の*評価者がその相関ブロックの偏りを暴露した場合にしか検出できない — 相関ブロックがEMのconsensusをそのまま乗っ取ってしまった場合は警告は出ない。自己整合性の診断であり、独立した正しさのチェックではない。`latent_truth_converged` は、EMループが収束epsilonに達する前にiteration上限を使い切った場合に false になる（`latent_truth_iterations` が上限に達し、`latent_truth_convergence_delta` は収束epsilonを上回ったまま）— その事後確率は依然として有効だが確信度は低い。バッチ全体でEMは1回だけ実行されるため、同じバッチ内の全サンプルで同じ値になる。 |

`MinRequirements` はどのモードでも閾値判定の**後に**適用されます。`--decision-score latent-truth` では、さらにその後にデフォルトで無効な3つの opt-in safety demotion が適用されます: `--demote-on-correlated-warning`（`correlated_evaluator_warning` が発火した場合に demote）、`--min-evaluator-effective-n <f64>`（`evaluator_effective_n` がこの絶対値を下回った場合に demote — 上記の相対的な70%閾値の警告とは独立）、`--demote-on-non-convergence`（`latent_truth_converged` が false の場合に demote）。いずれも `Keep` → `Review` へのみ demote します — これらは不確実性のシグナルであって間違いの確信ではないため、`Drop` まで落とすことはなく、`stability_score` 自体にも触れません。demote が発火した場合、`latent_truth_demotion_reason` がどの条件かを示します（`correlated_evaluator_warning`、`evaluator_effective_n_below_minimum`、`latent_truth_not_converged` のいずれか。複数条件が同時に発火した場合はこの優先順位）。`score` と `stable-wrong-risk` で利用可能。`calibrate`/`policy`/`compare --policy-after` は `decision` ではなく生のスコアを直接比較するため no-op です。

| 条件 | 判定 |
|------|------|
| スコア >= 0.85 | `keep`（採用） |
| スコア <= 0.40 | `drop`（除外） |
| それ以外 | `review`（要確認） |

これは *selective classification* / *reject option* / risk-coverage のトレードオフに対応します: すべてのサンプルを無理やりどちらかに分類するのではなく、不確かなものを review に回すことで、実際に採用する範囲のリスクを制御します。`keep` = accept、`review` = abstain/defer、`drop` = 学習用途としての reject。`calibrate`/`policy` はこれを実務化したものです（[calibrate コマンド](#calibrate-コマンド) 参照）が、閾値は1つの gold-labeled サンプルに対して経験的に選ばれるものであり、conformal のような厳密な保証ではありません。より誠実な（それでも経験的な）精度推定には `--heldout` を使ってください。詳細は [docs/metrics_ja.md](docs/metrics_ja.md#calibrate-と-policy-は-selective-classification-である)を参照。

`--keep-threshold` と `--drop-threshold` で閾値を変更できます。`--confidence-level` で Wilson LCB の信頼水準を調整できます（デフォルト 0.95）。

`--use-adjusted-score` と `--use-lcb-score` は `--decision-score adjusted` / `--decision-score lcb` の後方互換 alias です。`--decision-score` が指定された場合はそちらが優先されます。

## explain コマンド

サンプル1件の内訳を表示します。

```bash
quietset explain scored.jsonl --sample-id a
```

```
sample_id:          a
decision:           keep
n_observations:     3
stability_score:    0.9700
confidence:         0.5000
adjusted_score:     0.7350
label_agreement_lcb:0.4380
label_margin:       1.0000
label_entropy:      0.0000

score stats:
  mean:             0.8950
  std:              0.0150
  mad:              0.0150
  iqr:              0.0300

components:
  label                      1.0000  ████████████████████
  score_consistency          0.9850  ███████████████████
  budget_robustness          0.8800  █████████████████  ← weakest
  seed_robustness            0.9200  ██████████████████
```

`--json` フラグで `StabilityReport` の JSON をそのまま出力できます。

> **注意**: この例はデフォルトの raw スコア判定モードです（`stability_score = 0.97 → keep`）。`--use-adjusted-score` を使うと、n=3 での confidence ≈ 0.50 により `adjusted_score = 0.74` となり、`--keep-threshold` を下げない限り判定は **review** になります。`--use-lcb-score` では `label_agreement_lcb ≈ 0.44` となり、こちらも **review** になります。

## compare コマンド

2つの scored ファイルを `sample_id` で突合して前後比較します。

```bash
quietset compare before.jsonl after.jsonl
```

```
matched samples:  10000
mean stability:   0.7412 → 0.7801

decision transitions (before → after):
              →keep   →review    →drop
      keep↓    7210       311       42
    review↓     508      2101      301
      drop↓      19       104      404

top 5 regressions:
  sample_001  0.9100 → 0.4400  (Δ-0.4700)
  sample_382  0.8800 → 0.3900  (Δ-0.4900)
```

`--json` フラグでマシン可読な JSON を出力できます。モデル更新・プロンプト変更・評価器変更の前後比較に使えます。

`--components` フラグでコンポーネント別の平均差分を表示できます。

```bash
quietset compare before.jsonl after.jsonl --components
```

```
component deltas (mean before → after):
  label                      0.88 → 0.90  (+0.02)
  score_consistency          0.79 → 0.86  (+0.07)
  budget_robustness          0.91 → 0.72  (-0.19)  ← regression
```

`--json` 出力には `component_deltas` オブジェクト（符号付きデルタ値）が追加されます。

## summary コマンド

```bash
quietset summary scored.jsonl
```

```
samples:              1000
  keep:                621  (62.1%)
  review:              291  (29.1%)
  drop:                 88   (8.8%)
  lcb_keep_demotions:  139  (stability_score >= 0.85, label_agreement_lcb < 0.85)

stability_score:
  mean:              0.7412
  median:            0.7810
  p10 / p90:         0.4200 / 0.9600

score dispersion (mean across samples):
  mad:               0.0421
  iqr:               0.0812

top instability drivers (review + drop samples):
  label disagreement        38%
  score variance            24%
  seed sensitivity          21%
  budget sensitivity        17%
```

`lcb_keep_demotions` は `stability_score >= keep_threshold`（raw では keep）かつ `label_agreement_lcb < keep_threshold`（LCB では keep 未満）なサンプル数です。`--decision-score lcb` に切り替えたときに keep から降格されるサンプルの数を示します。raw スコアで既に review/drop なサンプルは除外されます。

`--json` フラグで CI/jq から使いやすい JSON を出力できます。

```bash
# drop 率が 10% 未満かチェック
quietset summary scored.jsonl --json | jq '.drop_rate < 0.1'
```

## reliability コマンド（experimental）

> 安定性は一致度であって正確性ではありません。`stable-wrong-risk` を使うと、安定的に誤ラベルな keep サンプルの割合を測定できます — 安定性フィルタリングの最も危険な失敗モードです。

観測 JSONL から評価者ごとの信頼度を推定します。

```bash
quietset reliability input.jsonl
```

```json
{"evaluator_id": "m1", "reliability": 0.94}
{"evaluator_id": "m2", "reliability": 0.71}
{"evaluator_id": "m3", "reliability": 0.52}
{"fleiss_kappa": 0.81, "krippendorff_alpha": 0.83}
```

各評価者のラベルが参照ラベルとどれだけ一致するかを計算します。デフォルトの参照ラベルは多数派ラベルですが、観測に `gold_label` が設定されている場合はそちらが優先されます。これにより、スコアリング結果を変えることなく正解ラベルベースの信頼度計算が可能です。信頼度が低い評価者の特定に使えます。

末尾の行はデータセット全体の一致度統計です。

| フィールド | 意味 |
|----------|------|
| `fleiss_kappa` | 偶然補正済みの評価者間一致係数（名義ラベル、サンプルごとに評価者数が異なっても対応）。0 = 偶然水準、1 = 完全一致、負 = 偶然以下。 |
| `krippendorff_alpha` | 一致行列方式の信頼性係数。名義ラベルに対応。kappa と同じスケール。 |

サンプルあたり2件以上の評価が2サンプル以上ない場合は出力されません（定義不能）。`jq 'select(.fleiss_kappa)'` でサマリ行だけ取り出せます。

`gold_label` がある場合、各評価者の行に `confusion` 混同行列も含まれます（`predicted → gold → count`）。

```json
{"evaluator_id": "m1", "reliability": 0.94, "confusion": {"win": {"win": 120, "loss": 8}, "loss": {"win": 11, "loss": 101}}}
{"fleiss_kappa": 0.81, "krippendorff_alpha": 0.83}
```

## filter コマンド（拡張）

`--min-stability`、`--max-disagreement`、`--decision` に加え、診断フィールドでの絞り込みができます。

| フラグ | 条件 |
|-------|------|
| `--min-label-lcb <f>` | `label_agreement_lcb >= f` |
| `--min-confidence <f>` | `confidence >= f` |
| `--max-score-mad <f>` | `score_mad <= f` |
| `--max-score-iqr <f>` | `score_iqr <= f` |

```bash
quietset filter scored.jsonl --min-label-lcb 0.70 --min-confidence 0.60 --max-score-mad 0.05 > clean.jsonl
```

## audit コマンド

scored JSONL の深掘り診断レポートを出力します。

```bash
quietset audit scored.jsonl
quietset audit scored.jsonl --json           # JSON 出力
quietset audit scored.jsonl --top 20         # リストを最大 20 件表示（デフォルト 10）
```

```
=== quietset audit ===
total:              1000
  keep:              621  (62.1%)
  review:            291  (29.1%)
  drop:               88   (8.8%)
  lcb_keep_demotions:  139  (stability >= 0.85, lcb < 0.85)

--- borderline (0.75 <= stability <= 0.95, top 10) ---
  sample_042  0.8201  review

--- high_raw_low_lcb (stability >= 0.85, lcb < 0.85, top 10) ---
  sample_003  stability=0.9100  lcb=0.3423

--- budget_sensitive (top 10) ---
  sample_091  budget_sensitivity=0.8200
```

`--json` 出力には `borderline`、`high_raw_low_lcb`、`high_score_mad`、`budget_sensitive`、`seed_sensitive` が配列で含まれます。

## calibrate コマンド

`gold_label` を持つ観測 JSONL から、精度目標を満たす `keep_threshold` を自動探索します。

```bash
quietset calibrate input.jsonl --target-precision 0.95
quietset calibrate input.jsonl --target-precision 0.98 --decision-score lcb
```

```json
{
  "decision_score": "lcb",
  "keep_threshold": 0.91,
  "drop_threshold": 0.40,
  "achieved_precision": 0.982,
  "precision_ci_low": 0.943,
  "precision_ci_high": 0.997,
  "coverage": 0.61,
  "n_keep": 610,
  "n_total": 1000,
  "validated_on_heldout": false,
  "note": "achieved_precision and its confidence interval were selected and measured on the same gold-labeled data; the threshold search makes this optimistic. Pass --heldout <path> for an independent estimate."
}
```

`keep_threshold` を 0.99 から 0.50 まで 0.01 刻みで探索し、精度目標を満たす最も緩い閾値を返します。`gold_label` がない場合や目標を達成できない場合はエラーになります（`--target-precision` を下げてください）。

`precision_ci_low`/`precision_ci_high` は `achieved_precision` に対する Wilson score interval で、`--confidence-level`（`lcb` decision score と同じノブ）を使う。gold ラベルが少ないと自動的に広がるので、`n_keep` が小さいときは `achieved_precision` 単体を正確な値として扱わないこと。この区間は選ばれた閾値における標本誤差のみをカバーしており、その閾値自体が同じ gold ラベルに対して目標を満たすように選ばれた（cherry-pick された）という事実は反映していない — 楽観的な値になる。下限の目安として扱い、より保守的に確認したい場合は held-out の gold セットを使うこと。`note` はこの注意を機械可読にしたもの: `--heldout` を使っていない場合は常に上記のように付与され、`--heldout` を使っていても選ばれた閾値での in-sample precision が held-out precision より 0.05 以上高い場合（過学習の疑い）に付与される。gap が小さく通常の標本誤差の範囲内なら `null` になる。

その「より保守的な確認」を実際に行うのが `--heldout <path>` です。`keep_threshold` の探索は引き続きメインの入力に対して行われますが、`achieved_precision`/`precision_ci_low`/`precision_ci_high`/`coverage`/`n_keep`/`n_total` はその閾値のまま held-out ファイル自身の `gold_label` で測定し直されます。`raw`/`adjusted`/`lcb`/`latent-truth` ではこれで循環参照が完全に解消されます — 閾値選択の循環は無くなり、latent-truth の EM はそもそも gold_label を一切使わないため、held-out の gold に対する評価は同じサンプルであっても真に独立した検証になります。`gold-weighted` だけは一段階循環が残ります — 評価者ごとの信頼度重みが held-out ファイル自身の `gold_label` から fit され、その同じラベルに対して採点されるため、in-sample の数値よりは良い指標ですが完全に独立ではないものとして扱ってください。stdin（`-`）には非対応（メイン入力が既に stdin を使うため）。held-out ファイルに `gold_label` が無いとエラーになります。メイン入力側で選ばれた閾値が過学習していれば、報告される precision は `--target-precision` を下回ることがありますが、それは正しい・正直なシグナルであり、バグではありません。出力の `validated_on_heldout` は `--heldout` が使われたかどうかを示すので、保存した結果を後で見ても何を測定したものか分かります。`train_precision` は同じ閾値での in-sample precision を `achieved_precision`（`--heldout` 使用時は held-out precision になる）と並べて公開します — `--heldout` を使っていない場合は `achieved_precision` と同じ値になってしまうため `null` になります:

```bash
quietset calibrate input.jsonl --target-precision 0.95 --heldout heldout_gold.jsonl
```
```json
{
  "decision_score": "raw",
  "keep_threshold": 0.99,
  "drop_threshold": 0.40,
  "achieved_precision": 0.71,
  "precision_ci_low": 0.44,
  "precision_ci_high": 0.89,
  "coverage": 1.0,
  "n_keep": 24,
  "n_total": 24,
  "train_precision": 0.97,
  "validated_on_heldout": true,
  "note": "in-sample precision at this threshold was 0.970, held-out precision is 0.710 (gap 0.260) — the training-selected threshold likely overfit the training gold labels"
}
```

`--output-format csv` でJSONオブジェクトの代わりにヘッダー行1つ＋データ行1つを出力できる:

```bash
quietset calibrate input.jsonl --target-precision 0.95 --output-format csv
```
```csv
decision_score,keep_threshold,drop_threshold,achieved_precision,precision_ci_low,precision_ci_high,coverage,n_keep,n_total,train_precision,validated_on_heldout,note
lcb,0.910000,0.400000,0.982000,0.943000,0.997000,0.610000,610,1000,,false,"achieved_precision and its confidence interval were selected and measured on the same gold-labeled data; the threshold search makes this optimistic. Pass --heldout <path> for an independent estimate."
```

`--fail-below-target` を使うと `calibrate` を CI gate にできます: 結果を出力した上で、`achieved_precision` が `--target-precision` を下回っていれば非ゼロで終了します。`--heldout` なしでは実質的に no-op です（grid search が既に主入力での `achieved_precision >= --target-precision` を保証するか、そうでなければエラーで失敗するため）。`--heldout` を使う場合、`achieved_precision` は held-out での測定値であり、目標を下回ることが正当にあり得ます — このフラグはその正直なシグナルを CI パイプラインが判定できる形にします:

```bash
quietset calibrate train.jsonl --heldout heldout.jsonl --target-precision 0.98 --fail-below-target
echo "exit code: $?"
```

## select コマンド

診断クラスでサンプルを抽出し、元の scored JSONL 行をそのまま出力します（パススルー、他コマンドへパイプ可能）:

```bash
quietset select scored.jsonl --class borderline --top 100
quietset select scored.jsonl --class high-raw-low-lcb > uncertain_keeps.jsonl
quietset select scored.jsonl --class budget-sensitive --top 20 | quietset explain - --sample-id x
```

| クラス | 抽出内容 |
|-------|---------|
| `borderline` | `keep_threshold ± 0.10` の安定性帯（不確実ゾーン） |
| `high-disagreement` | `disagreement_score` 降順 |
| `budget-sensitive` | `budget_sensitivity` 降順 |
| `seed-sensitive` | `seed_sensitivity` 降順 |
| `high-raw-low-lcb` | `stability_score >= keep_threshold` だが `label_agreement_lcb < keep_threshold` |
| `high-score-mad` | `score_mad` 降順 |

`--top N` で出力数を制限します。`--keep-threshold` で `borderline`/`high-raw-low-lcb` の帯を調整できます（デフォルト 0.85）。

## recommend コマンド

問題が検出された各サンプルに対し、優先順位付きで再評価の提案を出力します:

```bash
quietset recommend scored.jsonl
quietset recommend scored.jsonl --unstable-only   # 問題のない keep をスキップ
```

```json
{"sample_id": "x42", "reason": "high_raw_low_lcb", "recommended_action": "add_observations", "stability_score": 0.91, "label_agreement_lcb": 0.34, "n_observations": 2}
{"sample_id": "y17", "reason": "high_seed_sensitivity", "recommended_action": "add_seeds", "seed_sensitivity": 0.71}
```

| 理由 | 対応 |
|--------|--------|
| `high_raw_low_lcb` | raw の安定性は高いが LCB が閾値未満 → `add_observations` |
| `low_evaluator_agreement` | evaluator_agreement < 0.7 → `add_evaluators` |
| `high_seed_sensitivity` | seed_sensitivity > 0.3 → `add_seeds` |
| `high_budget_sensitivity` | budget_sensitivity > 0.3 → `increase_budget` |
| `low_model_agreement` | model_agreement < 0.7 → `add_models` |
| `low_score_consistency` | score_consistency < 0.7 → `reduce_score_variance` |

各サンプルにつき最大1件の提案（優先度が最も高いルールが採用されます）。

## stable-wrong-risk コマンド

観測 JSONL を内部でスコアリングし、`majority_label` が `gold_label` と異なる keep サンプルを報告します:

```bash
quietset stable-wrong-risk input.jsonl
quietset stable-wrong-risk input.jsonl --keep-threshold 0.90
```

```json
{
  "n_total": 1000,
  "n_keep": 621,
  "n_stable_wrong": 12,
  "stable_wrong_rate_among_keep": 0.019,
  "samples": [
    {"sample_id": "x42", "stability_score": 0.96, "majority_label": "loss", "gold_label": "win"}
  ]
}
```

`gold_label` が必須です。`stability_score` 降順にソートされ、最も自信を持って keep されている誤りサンプルが先頭に来ます。`--top N` でサンプル数を制限できます。

`--breakdown` を使うと、`Keep` 判定サンプルにおける `stable_wrong_rate` を `evaluator_id`/`model_id`/`budget` ごとに分解して報告できます — 特定の評価者・モデル・budget が「安定して間違っているのに keep されているサンプル」に不釣り合いに多く現れていないかを見つけられます:

```bash
quietset stable-wrong-risk input.jsonl --breakdown
```
```json
{
  "n_total": 1000,
  "n_keep": 621,
  "n_stable_wrong": 12,
  "stable_wrong_rate_among_keep": 0.019,
  "samples": [ ... ],
  "by_evaluator": [
    {"evaluator_id": "m1", "n_keep": 340, "n_stable_wrong": 9, "stable_wrong_rate": 0.026},
    {"evaluator_id": "m2", "n_keep": 281, "n_stable_wrong": 3, "stable_wrong_rate": 0.011}
  ],
  "by_model": [ ... ],
  "by_budget": [ ... ]
}
```

各内訳はトップレベルの rate と同様サンプルレベルです: `by_evaluator[i].n_keep` はその `evaluator_id` に触れている `Keep` サンプル数であり、観測数ではありません。1つのサンプルが複数の evaluator/model/budget に同時に触れることがあるため、内訳の各行を合計してもトップレベルの `n_keep` にはなりません — これは意図的なもので、パーティション（分割）ではありません。「wrong-but-confident なサンプルにどの evaluator/model/budget が現れるか」を答えるものであり、「誰の責任か」を答えるものではありません。デフォルトでは無効です — 入力をクローンして追加のグルーピング処理を行うためです。

## compare --policy-after

通常の比較出力に加えて、after ファイルの判定が仮の decision-score ポリシーの下でどう変わるかを表示します:

```bash
quietset compare before.jsonl after.jsonl --policy-after lcb
quietset compare before.jsonl after.jsonl --policy-after adjusted --policy-keep-threshold 0.80
```

```
policy comparison: current → lcb (keep_threshold=0.85):
              →keep   →review    →drop
    keep↓         0       850        0
  review↓         0      2291      300
    drop↓         0         0      200
  demoted by policy: 850  promoted: 0
```

> **注意:** `--policy-after lcb` は `label_agreement_lcb` を LCB ポリシースコアの近似値として使用します。他のコンポーネントは再計算されないため、結果は近似値です。正確な予測ではなく方向性の参考（「ポリシーによって何件が降格されるか」）としてお使いください。

## policy コマンド

`keep_threshold` を 0.99 から 0.50 まで掃引し、各段階での precision/coverage/stable-wrong-rate のトレードオフを表示します。`score` を実行する前に閾値を選ぶのに使えます:

```bash
quietset policy input.jsonl
quietset policy input.jsonl --target-precision 0.95
quietset policy input.jsonl --decision-score lcb --json
```

```
threshold  n_keep  coverage
0.99            1     0.500
0.98            1     0.500
0.97            1     0.500
```

観測に `gold_label` があれば、表に `precision` と `stable_wrong_rate` の列も追加されます。`--target-precision`/`--target-coverage` は目標を満たす最も緩い閾値を `←` で示します。`--output-format json` は整形テーブルの代わりに閾値ごとに1行のJSONLを出力します(`--json` も引き続きエイリアスとして使用可。両方指定すると警告が出て `--output-format` が優先される)。`--output-format csv` は閾値ごとに1行、固定列 `threshold,n_keep,coverage,precision,stable_wrong_rate,best` で出力し、`gold_label` が無い場合は `precision`/`stable_wrong_rate` が空、`best` は `--target-precision`/`--target-coverage` に一致する行だけ `true`:

```bash
quietset policy input.jsonl --target-precision 0.95 --output-format csv
```
```csv
threshold,n_keep,coverage,precision,stable_wrong_rate,best
0.99,1,0.500000,1.000000,0.000000,
0.98,1,0.500000,1.000000,0.000000,true
```

## active-review コマンド

scored JSONL を、低い `label_agreement_lcb`・高い `label_entropy`・高い `score_mad`・高い budget/seed 感度を重み付け合成した「再評価の緊急度」でランク付けします:

```bash
quietset score input.jsonl | quietset active-review -
quietset active-review scored.jsonl --unstable-only --top 20
```

```json
{"budget_sensitivity":0.625,"cost":1.0,"expected_coverage_gain":1.0,"expected_risk_reduction":0.0,"label_agreement_flip_ratio":0.818,"label_agreement_lcb":0.301,"label_entropy":0.811,"primary_reason":"high_entropy","sample_id":"b","seed_sensitivity":0.075,"suggested_action":"add_evaluator","urgency_score":0.502,"utility":0.818}
```

`--unstable-only` は問題のない keep サンプルをスキップします。信号ごとの `--weight-*` フラグ（`--weight-lcb`、`--weight-entropy`、`--weight-score-mad`、`--weight-budget-sensitivity`、`--weight-seed-sensitivity`）で、レビュー予算に応じて重視する信号を調整できます。`suggested_action` は `request_gold_label`、`add_evaluator`、`add_model`、`increase_budget`、`add_seed` のいずれかです（以前の `add_observations`/`diversify_evaluators`/`reduce_score_variance`/`add_budget`/`add_seeds` からの改名 — 破壊的変更、CHANGELOG参照）。

5つの新しいフィールドが、緊急度ヒューリスティックを期待値ベースのランキングに拡張します:

- `label_agreement_flip_ratio` — `[0.0, 1.0]` の閾値距離比: `--keep-threshold`/`--drop-threshold` ちょうどで `1.0`、そこから離れるほど `label_agreement` 自身の Wilson CI 誤差幅に対して相対的に `0.0` へ縮む。**確率ではない** — モデル化されているのは `label_agreement` の標本誤差だけであり、`stability_score` の他の構成要素（スコア分散、budget/seed感度、model/evaluator agreement）にはこのコードベースに不確実性モデルが無いため、それらが実際に `stability_score` を左右している場合はこの比率は粗い近似にすぎない。ラベルが無いサンプルでは `None`。
- `expected_coverage_gain` — サンプルの `decision`（入力からそのまま読まれ、再計算されない）が `keep` でない場合は `1 / n_total`、そうでなければ `0.0`: このサンプルの判定が `keep` に反転した場合の正確な coverage 変化量。
- `expected_risk_reduction` — サンプルが `keep` かつ `label_agreement_lcb` が `--keep-threshold` を下回る場合（`stable-wrong-risk` の `lcb_keep_demotions` と同じ「risk のある keep」条件）は `1 / n_keep`、そうでなければ `0.0`。
- `cost` — `suggested_action` のコスト。`--cost-seed`/`--cost-budget`/`--cost-evaluator`/`--cost-gold-label`（すべてデフォルト `1.0`。`--cost-evaluator` は `add_model` のコストも兼ねる）から。
- `utility` — `label_agreement_flip_ratio * (expected_coverage_gain + expected_risk_reduction) / cost`。文字通りの期待値ではなくランキングスコア — これで順位付けすること自体は妥当だが（閾値に近く・見返りが大きく・安く直せるものほど上位に来る）、絶対値を較正された期待値として読んではいけない。

`--keep-threshold`/`--drop-threshold`（デフォルトは `score` と同じ `0.85`/`0.40`）は `label_agreement_flip_ratio` と at-risk-keep のゲート判定にのみ使われ、`decision` を再計算するものではない。`--rank-by urgency`（デフォルト）は `urgency_score` で、`--rank-by utility` は `utility` でソートし、単に不安定に見えるサンプルではなく追加レビューの効果が最も大きいサンプルを浮かび上がらせる。

`--plan <path>` を使うと、予算制約付きの評価計画を JSONL として書き出せる — `--rank-by` に関わらず常に `utility` 降順で、通常の標準出力に加えて出力される。`--budget <f64>` は含めるエントリの総 `cost` の上限を指定する: utility 順に貪欲に採用し、収まらないものはスキップして（そこで打ち切らずに）走査を続けるので、リストの後方にあるより安価で utility の低いエントリも含められる可能性がある。`--budget` を省略すると、`--plan` は全エントリを（utility 順のまま）書き出す:

```bash
quietset active-review scored.jsonl --plan next_eval_plan.jsonl --budget 20.0
```

これは knapsack 型の予算配分問題に対する標準的な value-density 貪欲近似であり、大域最適は保証されないが、「次のレビュー予算を何に使うべきか」の順位付けとしては妥当なものになる。

`--observations <path>`（`score` を実行したのと同じ生の observation JSONL）を渡すと、`suggested_action` に対する具体的な target 値も計算される — `StabilityReport` には派生統計（sensitivity/robustness スコア）しかなく、target をサイジングするための生の budget/seed/evaluator/model 値が失われているため。`audit --observations` が agreement 統計のために埋めているのと同じギャップです:

```bash
quietset active-review scored.jsonl --observations input.jsonl
```

```json
{"cost":1.0,"expected_coverage_gain":1.0,"expected_risk_reduction":0.0,"label_agreement_flip_ratio":0.766,"label_agreement_lcb":0.510,"label_entropy":0.0,"primary_reason":"high_seed_sensitivity","sample_id":"pricey","seed_sensitivity":0.8,"suggested_action":"add_seed","target_seed":2,"urgency_score":0.423,"utility":0.766}
```

`suggested_action` に対応するフィールドだけが、そのサンプル自身の observation から計算される:

- `increase_budget` → `target_budget` = そのサンプルで観測された最大 `budget` × 2
- `add_seed` → `target_seed` = そのサンプルで観測された最大 `seed` + 1
- `add_evaluator` → `target_evaluator_slot` = そのサンプルの distinct `evaluator_id` 数 + 1
- `add_model` → `target_model_slot` = そのサンプルの distinct `model_id` 数 + 1
- `request_gold_label` → target field なし（サイジングする対象がないため）

`--observations` を渡さない場合はすべて省略され、observation ファイルに該当する生の値が存在しない場合もサンプルごとに省略される — 捏造した既定値になることはない。

## block-score コマンド

観測を`sample_id`ではなく`block_id`（例: 32局面の学習ブロック）でグループ化し、各ブロックの軌道安定性を分類します:

```bash
quietset block-score observations.jsonl
```

```json
{"block_id":"B5","n_samples":2,"n_observations":4,"seed_effect_consistency":1.0,"shuffle_direction_consistency":1.0,"checkpoint_reproducibility":1.0,"block_stability":1.0,"trajectory_effect_mean":-0.45,"dead_unit_rate":0.5,"classification":"stable_shrink"}
```

`block_stability = seed_effect_consistency × shuffle_direction_consistency ×
checkpoint_reproducibility` — 計算可能な因子（それぞれ対応する軸`seed`/`shuffle_seed`/`model_id`で2つ以上の異なるグループが必要）の積です。`classification`は以下のいずれかです:

| クラス | 意味 |
|-------|------|
| `stable_growth` | `block_stability >= --stable-threshold` かつ `trajectory_effect_mean`の正味が正 |
| `stable_shrink` | `block_stability >= --stable-threshold` かつ `trajectory_effect_mean`の正味が負 |
| `seed_sensitive` | 不安定で、`seed_effect_consistency`が閾値未満の因子の中で最も低い |
| `order_sensitive` | 不安定で、`shuffle_direction_consistency`が閾値未満の因子の中で最も低い |
| `trajectory_sensitive` | 不安定で、`checkpoint_reproducibility`が閾値未満の因子の中で最も低い |
| `pathological` | `dead_unit_rate`/`saturated_unit_rate`が`--pathological-dead-rate`を超える — 他のすべての信号より優先される |
| `insufficient` | 条件軸の多様性が不足している（または成長/縮小の符号を読み取る`trajectory_effect`が無い） |

**`stable_growth`/`stable_shrink`は「善」「悪」のラベルではありません** — これらは`trajectory_effect`の符号の再現性を表すものであり、その効果が望ましいかどうかを表すものではありません。成長が常に改善とは限らず、縮小が常に悪化とは限りません（dead/saturatedユニット率の上昇を伴う縮小は別途`pathological`として捕捉されますが、健全な縮小は健全な正則化かもしれません）。自身のheld-out/downstream検証が完了するまでの推奨運用は以下の通りです:

| クラス | 推奨される処理 |
|-------|---------------|
| `pathological` | 学習から除外 |
| `insufficient` | 判断前に追加観測を収集 |
| `seed_sensitive` | seedを追加して再分類 |
| `order_sensitive` | shuffle seedを追加して再分類 |
| `trajectory_sensitive` | 別のcheckpointで再評価 |
| `stable_growth` | downstream検証へ送る——自動keepしない |
| `stable_shrink` | downstream検証へ送る——自動除外しない |

`--stable-threshold`（デフォルト`0.85`）、`--sensitivity-threshold`（デフォルト`0.5`）、`--pathological-dead-rate`（デフォルト`0.5`）が分類を調整します。`score`の`--keep-threshold`/`--drop-threshold`と同じく「運用上のデフォルト値であり、導出された定数ではない」という位置づけです——**何かのキャリブレーションされた確率ではありません**（特に、対局の棋力向上に対するキャリブレーションされた確率ではありません）。無人でこの分類に学習データの選別を任せる前に、自分自身のheld-outデータと対局結果で検証してください。3つの一貫性因子がなぜ平均ではなく積になっているかは[docs/metrics.md](docs/metrics.md)を参照してください。`block_id`を持つ観測が1つもない場合はstderrに警告を出します（エラーにはなりません）。

## trajectory-audit コマンド

before/after の観測ファイル（例: 2つの学習checkpoint）間でブロック単位の軌道安定性を比較します — 破壊的サンプル検出器です:

```bash
quietset trajectory-audit --before checkpoint_143.jsonl --after checkpoint_176.jsonl --group-size 32
```

```
B5  stable_shrink → pathological
  dead_unit_delta[ft]:       +1.0000
  trajectory_effect_delta:      -0.4500
```

`block_id`でブロックを突合し（片側にしか存在しないブロックはスキップ）、突合できた各ブロックについて次を報告します: `dead_unit_delta`/`saturated_unit_delta`（`layer_id`ごとに`after_rate - before_rate`。片側にしか存在しないレイヤーはもう片方を`0.0`として扱うため、新規に問題化したレイヤーもその`after_rate`がそのままdeltaとして表示される）、`trajectory_effect_delta`、`before_classification`/`after_classification`、`reproducibility_3seed`（after側で`seed`が3種類以上存在する場合のみの`seed_effect_consistency`）。dead/saturatedユニットのdelta絶対値が大きいブロックから順にソートされます。`--json`でマシン可読なJSONL出力（ブロックごとに1オブジェクト）。

`--group-size N`は、どちらかの側で`block_id`ごとの distinct `sample_id`数が`N`と一致しないブロックについてstderrに警告します — これは破壊的な学習の兆候ではなく、ブロックのグルーピングが壊れている（例: 32局面ブロックが途中で切れている）ことを示します。`block-score`と同じ`--stable-threshold`/`--sensitivity-threshold`/`--pathological-dead-rate`フラグを持ちます。

## Rust API

```rust
use quietset::{Observation, ScoreConfig, score_all};

let obs = vec![
    Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.9), ..Default::default() },
    Observation { sample_id: "a".into(), label: Some("win".into()), score: Some(0.88), ..Default::default() },
];
let reports = score_all(obs, &ScoreConfig::default());
println!("{:?}", reports[0].decision);
```

### ストリーミング API

観測データが `sample_id` でソート済みの場合、`StreamingScorer` でメモリ効率よく処理できます。

```rust
use quietset::{Observation, ScoreConfig, StreamingScorer};

let mut scorer = StreamingScorer::new(ScoreConfig::default());
for obs in observations {
    if let Some(report) = scorer.push(obs) { println!("{:?}", report.decision); }
}
if let Some(report) = scorer.flush() { println!("{:?}", report.decision); }
```

## Python バインディング

`crates/quietset-py` に [pyo3](https://pyo3.rs/) + [maturin](https://www.maturin.rs/) を使った Python バインディングがあります。状態: **alpha / experimental** — コアのスコアリング API はラップ済みだが、インターフェースは今後変わる可能性がある。Cargo workspace および CI からは除外されており（`cargo build/test --workspace` では一切触れない）、`crates/quietset-py` 内で個別にビルド・テストする必要がある。

```bash
cd crates/quietset-py
pip install maturin
maturin develop
```

```python
import quietset
result = quietset.score_jsonl('{"sample_id":"a","label":"win","score":0.9}\n' * 2)
print(result)
```

バインディングは現在 `score_jsonl` という単一の関数のみを公開しており、JSONL 文字列をデフォルト設定でスコアリングして JSONL 文字列を返す。CLI が全コマンド・全オプションを備えた安定インターフェースであり、Python バインディングはサブプロセス起動が難しい既存の Python パイプラインに簡易スコアリングを組み込む用途向け。

## これらの数値の根拠

quietset は単一の論文の実装ではありません。サンプルごとの基礎指標の多くは既存の、名前の付いた統計量です。一方で `stability_score` とそれを組み合わせた keep/review/drop の判定ポリシーは quietset 独自の設計であり、理論的な保証ではありません。この表はコンパクトな分類です — 各指標の完全な数式・前提・failure mode は [docs/metrics_ja.md](docs/metrics_ja.md) を参照してください。

**既存の統計量**（忠実な実装。完全な数式はリンク先の各セクションを参照）:

| フィールド | 根拠 |
|-----------|------|
| `label_agreement_lcb` | Wilson score interval の下側信頼限界 — [安定性スコア](#安定性スコアstability_score) 参照 |
| `fleiss_kappa` | Fleiss' kappa（サブジェクトごとの評価者数が可変な場合に一般化） — [reliability](#reliability-コマンドexperimental) 参照 |
| `krippendorff_alpha` | Krippendorff's alpha（nominal ラベル向けバリアント） — [reliability](#reliability-コマンドexperimental) 参照 |
| `label_entropy` | そのサンプルで観測されたラベルに対する正規化 Shannon entropy — [安定性スコア](#安定性スコアstability_score) 参照 |
| `score_std` / `score_mad` / `score_iqr` | 母標準偏差 / 中央値ベースの MAD / 線形補間による IQR — [安定性スコア](#安定性スコアstability_score) 参照 |

**モデルベースの推定量:**

| フィールド | 根拠 |
|-----------|------|
| `latent_truth_*` | Dawid-Skene 系 EM — 評価者ごとの混同行列とラベル事後分布を `gold_label` なしで推定する。`evaluator_effective_n` と `correlated_evaluator_warning` は、収束した EM の出力の上に quietset が独自に追加した診断指標であり、オリジナルの Dawid-Skene 手法の一部ではない — [判定](#判定decision) 参照 |

**経験的診断指標**（`gold_label` に対して直接計測。モデル化はしていない）:

| フィールド | 根拠 |
|-----------|------|
| `stable_wrong_rate_among_keep` | 直接計測される経験的な率 — [stable-wrong-risk](#stable-wrong-risk-コマンド) 参照 |
| `stable-wrong-risk --breakdown` | 同じ率を evaluator/model/budget 別に集計したもの — [stable-wrong-risk](#stable-wrong-risk-コマンド) 参照 |

**quietset 独自のヒューリスティック**（上記を組み合わせて運用上の判定にしたもの。単体では引用可能な統計手法ではない）:

| フィールド | 根拠 |
|-----------|------|
| `stability_score` | 上記サブスコアの加重平均 — [安定性スコア](#安定性スコアstability_score) 参照 |
| `confidence` / `adjusted_stability_score` | 証拠数に基づく中立値 0.5 への shrinkage — [confidence と adjusted_stability_score](#confidence-と-adjusted_stability_score) 参照 |
| `score_sign_agreement` | 符号付き・ゼロ中心のスコア向けの実務寄り補完指標 — [安定性スコア](#安定性スコアstability_score) 参照 |
| `keep`/`drop` 閾値（0.85 / 0.40） | 運用上のデフォルト値であり、数式から導出された定数ではない — [判定](#判定decision) 参照 |

`calibrate`/`policy` は selective prediction / risk-coverage（reject option）の考え方に近いです（不確かなサンプルを reject して review に回すことで、採用範囲のリスクを下げる）。ただし **conformal prediction のような厳密な保証ではありません**。閾値は1つの gold-labeled サンプルに対して経験的に選ばれるため、`achieved_precision` は「証明された値」ではなく「計測された値」として扱ってください。詳細は [calibrate](#calibrate-コマンド) の in-sample-optimism に関する `note` と `--heldout` の説明を参照。

## 類似ツールとの比較

| ツール | 何をするか | quietset との違い |
|--------|-----------|------------------|
| **Cleanlab** | 訓練済み分類器と Confident Learning でラベルエラーを検出。 | quietset はモデル訓練不要で、タスク固有の前提を持たない。推定ラベル品質ではなく安定性でフィルタリングする。 |
| **Label Studio** | 画像・テキスト・音声のラベリング用 Web アノテーションプラットフォーム。 | quietset は CLI/ライブラリのプリミティブであってアノテーション UI ではない。 |
| **pandas / polars** | 汎用データ操作ライブラリ。 | quietset は `keep / review / drop` 判定・次元別サブスコア・confidence・不安定要因診断を一括提供する。 |
| **Great Expectations / Soda** | スキーマ・値域ルールに対するデータ品質検証。 | これらは「スキーマ適合性」を検査する。quietset は「繰り返し評価をまたいだ一致性」を検査する。 |
| **scipy.stats / sklearn metrics** | Cohen's kappa・Fleiss' kappa などの統計関数。 | quietset は同様のアイデアを JSONL I/O・confidence 調整・閾値設定・パイプライン合成付きのプリミティブとして提供する。 |
| **LLM 評価フレームワーク（RAGAS、DeepEval）** | モデルベースの judge で LLM 出力を評価。 | quietset は judge に依存しない。judge の出力を受け取り、実行・budget・モデルをまたいだ合意度を計測する。 |

## ライセンス

MIT OR Apache-2.0
