# quietset

quietset は、タスク固有の前提条件に依存せず、ラベルの安定性によってデータセットをフィルタリングします。

複数の評価者・計算コスト・ランダムシード・モデルチェックポイント・繰り返し実行を通じて、ラベルやスコアが安定しているサンプルだけを残すために使います。

ノイズの多い教師データの整理、合成データのフィルタリング、強化学習のサンプル選択、探索ベースのラベリング、シミュレーション結果の絞り込み、ベンチマークのキュレーションに役立ちます。

quietset はモデルの訓練ツールでも、アノテーションプラットフォームでも、画像品質チェッカーでもありません。他のツールと組み合わせて使う、小さな安定性フィルタリングのプリミティブです。

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

# 最低3件の観測・2人の評価者がなければ keep にしない
quietset score input.jsonl --min-observations-keep 3 --min-evaluators-keep 2 > scored.jsonl
```

## 入力 JSONL フォーマット

`sample_id` 以外のフィールドはすべて省略可能です。あるフィールドだけを含むサブスコアのみが安定性計算に使われます。

```json
{"sample_id":"a","label":"win","score":0.91,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"a","label":"win","score":0.88,"evaluator_id":"m1","budget":8,"seed":1}
{"sample_id":"b","label":"win","score":0.52,"evaluator_id":"m1","budget":4,"seed":1}
{"sample_id":"b","label":"loss","score":-0.10,"evaluator_id":"m2","budget":8,"seed":2}
```

## 出力 JSONL フォーマット

主要フィールド（計算できない場合は省略されます）:

```json
{
  "sample_id": "a",
  "n_observations": 2,
  "majority_label": "win",
  "label_agreement": 1.0,
  "label_margin": 1.0,
  "label_entropy": 0.0,
  "score_mean": 0.895,
  "score_std": 0.015,
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

### 用途別に重みを調整する

```bash
# LLM judge: 評価者・モデル間の合意を重視
quietset score input.jsonl --weight-evaluators 2.0 --weight-models 2.0

# ゲーム探索・シミュレーション: シード・budget への頑健性を重視
quietset score input.jsonl --weight-seed 2.0 --weight-budget 2.0
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

デフォルトでは `stability_score` が判定に使われます。`--use-adjusted-score` を指定すると
`adjusted_stability_score` が使われます。`MinRequirements` はどちらのモードでも
閾値判定の**後に**適用され、上書きされることはありません。

| 条件 | 判定 |
|------|------|
| スコア >= 0.85 | `keep`（採用） |
| スコア <= 0.40 | `drop`（除外） |
| それ以外 | `review`（要確認） |

`--keep-threshold` と `--drop-threshold` で閾値を変更できます。

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
label_margin:       1.0000
label_entropy:      0.0000

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

## summary コマンド

```bash
quietset summary scored.jsonl
```

```
samples:              1000
  keep:                621  (62.1%)
  review:              291  (29.1%)
  drop:                 88   (8.8%)

stability_score:
  mean:              0.7412
  median:            0.7810
  p10 / p90:         0.4200 / 0.9600

top instability drivers (review + drop samples):
  label disagreement        38%
  score variance            24%
  seed sensitivity          21%
  budget sensitivity        17%
```

`--json` フラグで CI/jq から使いやすい JSON を出力できます。

```bash
# drop 率が 10% 未満かチェック
quietset summary scored.jsonl --json | jq '.drop_rate < 0.1'
```

## reliability コマンド（experimental）

観測 JSONL から評価者ごとの信頼度を推定します。

```bash
quietset reliability input.jsonl
```

```json
{"evaluator_id": "m1", "reliability": 0.94}
{"evaluator_id": "m2", "reliability": 0.71}
{"evaluator_id": "m3", "reliability": 0.52}
```

各評価者のラベルがサンプルの多数派ラベルとどれだけ一致するかを計算します。信頼度が低い評価者を特定するために使えます。

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
