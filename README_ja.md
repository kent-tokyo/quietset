# quietset

[![CI](https://github.com/kent-tokyo/quietset/actions/workflows/ci.yml/badge.svg)](https://github.com/kent-tokyo/quietset/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/quietset.svg)](https://crates.io/crates/quietset)
[![docs.rs](https://docs.rs/quietset/badge.svg)](https://docs.rs/quietset)

モデルに依存しない安定性フィルタ — 評価者・budget・seed・モデルチェックポイントをまたいでラベルやスコアが一致するサンプルだけを残します。

quietset はモデルの訓練ツールでも、アノテーションプラットフォームでも、画像品質チェッカーでもありません。他のツールと組み合わせて使う、小さな安定性フィルタリングのプリミティブです。

> **注意:** quietset が測るのは「安定性」であり「正確性」ではありません。評価者が一致して間違えるサンプルは高スコアになります。`gold_label` ベースの reliability や `--decision-score lcb` を使って証拠量ベースの保守性を加えてください。

## ユースケース

### ゲーム AI / 探索学習データ

複数エンジン・探索深さ・seed で同じ局面を評価し、評価値やラベルが安定している局面だけを学習データに残す。

```bash
quietset score positions.jsonl --profile game-ai > stable_positions.jsonl
quietset stable-wrong-risk positions.jsonl  # 安定して誤ラベルの局面を検出
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

## 入力 JSONL フォーマット

`sample_id` 以外のフィールドはすべて省略可能です。あるフィールドだけを含むサブスコアのみが安定性計算に使われます。

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1,"gold_label":"win"}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1,"gold_label":"win"}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

`gold_label` はサンプルの既知の正解ラベルです。指定されている場合、`reliability` コマンドが多数派ラベルの代わりにこれを正解として使います。

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

利用可能なサブスコアの**加重平均**として計算されます（すべて `[0.0, 1.0]`）。

| コンポーネント | 意味 |
|--------------|------|
| `label_agreement` | 多数派ラベルを持つ観測の割合 |
| `score_consistency` | `1 - 正規化スコア標準偏差` |
| `budget_robustness` | `1 - budget_sensitivity`（計算コスト変化への頑健性） |
| `seed_robustness` | `1 - seed_sensitivity`（シード変化への頑健性） |
| `model_agreement` | モデル間のラベル一致度 |
| `evaluator_agreement` | 評価者間のラベル一致度 |

対応するフィールドがない次元（例：ラベルなし、budget なし）は平均から除外されます。
観測が1件のみのサンプルは `stability_score = 0.5`（デフォルトで `review`）になります。

追加の診断フィールド:

| フィールド | 意味 |
|----------|------|
| `label_margin` | `(多数派数 - 2位数) / 合計`。0.0 = 完全に拮抗 |
| `label_entropy` | 正規化シャノンエントロピー [0, 1]。1.0 = ラベルが均等分布 |
| `label_agreement_lcb` | `label_agreement` の Wilson 信頼区間下限。少数観測の過信を防ぐ保守的な指標。 |
| `score_mad` | スコアの中央絶対偏差（MAD）。外れ値への耐性が `score_std` より高い。 |
| `score_iqr` | スコアの四分位範囲（Q3 − Q1）。 |
| `budget_slope` | budget 増加に対するスコアの傾き（正 = 上昇収束） |
| `confidence` | `n / (n + k)` — 証拠量に基づくスコアの信頼度 |
| `adjusted_stability_score` | `stability * confidence + 0.5 * (1 - confidence)` |

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

手動で重みを調整する代わりに、`--profile` でユースケースプリセットを適用できます。
明示的な `--weight-*` / `--decision-score` はプリセットより優先されます。

| プロファイル | 重み変更 | デフォルト decision-score |
|------------|--------|------------------------|
| `llm-judge` | evaluator ×2、model ×2 | `lcb` |
| `simulation` | budget ×2、seed ×2 | `adjusted` |
| `game-ai` | budget ×2、seed ×1.5、最低観測数 3 | `adjusted` |
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

`--use-adjusted-score` フラグで adjusted score ベースの判定に切り替えられます。
`--confidence-k` で収束速度を調整できます。

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

デフォルトでは `stability_score` が判定に使われます。3つのモードがあります。

| フラグ | alias | 使用スコア | 特性 |
|-------|-------|----------|------|
| `--decision-score raw` *（デフォルト）* | — | `stability_score` | 生の安定性。速いが少数観測の過信リスクあり。 |
| `--decision-score adjusted` | `--use-adjusted-score` | `adjusted_stability_score` | 証拠量に比例してペナルティ。 |
| `--decision-score lcb` | `--use-lcb-score` | `label_agreement_lcb`（ラベル部分） | Wilson LCB — 最も保守的。2/2 一致でも LCB ≈ 0.34（95%信頼）になり、証拠なしでは keep にならない。 |

`MinRequirements` はどのモードでも閾値判定の**後に**適用されます。

| 条件 | 判定 |
|------|------|
| スコア >= 0.85 | `keep`（採用） |
| スコア <= 0.40 | `drop`（除外） |
| それ以外 | `review`（要確認） |

`--keep-threshold` と `--drop-threshold` で閾値を変更できます。`--confidence-level` で
Wilson LCB の信頼水準を調整できます（デフォルト 0.95）。

`--use-adjusted-score` と `--use-lcb-score` は `--decision-score adjusted` / `--decision-score lcb` の
後方互換 alias です。`--decision-score` が指定された場合はそちらが優先されます。

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

> **注意**: この例はデフォルトの raw スコア判定モードです（`stability_score = 0.97 → keep`）。
> `--use-adjusted-score` を使うと、n=3 での confidence ≈ 0.50 により `adjusted_score = 0.74` となり、
> `--keep-threshold` を下げない限り判定は **review** になります。
> `--use-lcb-score` では `label_agreement_lcb ≈ 0.44` となり、こちらも **review** になります。

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

`lcb_keep_demotions` は `stability_score >= keep_threshold`（raw では keep）かつ
`label_agreement_lcb < keep_threshold`（LCB では keep 未満）なサンプル数です。
`--decision-score lcb` に切り替えたときに keep から降格されるサンプルの数を示します。
raw スコアで既に review/drop なサンプルは除外されます。

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

サンプルあたり2件以上の評価が2サンプル以上ない場合は出力されません（定義不能）。
`jq 'select(.fleiss_kappa)'` でサマリ行だけ取り出せます。

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
  "coverage": 0.61,
  "n_keep": 610,
  "n_total": 1000
}
```

`keep_threshold` を 0.99 から 0.50 まで 0.01 刻みで探索し、精度目標を満たす最も緩い閾値を返します。
`gold_label` がない場合や目標を達成できない場合はエラーになります（`--target-precision` を下げてください）。

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

`crates/quietset-py/` に pyo3 + maturin を使ったバインディングがあります。

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
