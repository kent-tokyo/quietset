# quietset

quietset は、タスク固有の前提条件に依存せず、ラベルの安定性によってデータセットをフィルタリングします。

複数の評価者・計算コスト・ランダムシード・モデルチェックポイント・繰り返し実行を通じて、ラベルやスコアが安定しているサンプルだけを残すために使います。

ノイズの多い教師データの整理、合成データのフィルタリング、強化学習のサンプル選択、探索ベースのラベリング、シミュレーション結果の絞り込み、ベンチマークのキュレーションに役立ちます。

quietset はモデルの訓練ツールでも、アノテーションプラットフォームでも、画像品質チェッカーでもありません。他のツールと組み合わせて使う、小さな安定性フィルタリングのプリミティブです。

## インストール

```bash
cargo install --path crates/quietset-cli
```

crates.io から直接インストールする場合:

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

# CSV 出力（pandas・R との連携に便利）
quietset score input.jsonl --output-format csv > scored.csv

# ラベル合意を2倍重視、スコア分散を無視
quietset score input.jsonl --weight-labels 2.0 --weight-scores 0.0 > scored.jsonl

# 不正な行をスキップ（エラーで止めない）
quietset score input.jsonl --skip-invalid > scored.jsonl
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

```json
{"sample_id":"a","n_observations":2,"majority_label":"win","label_agreement":1.0,"score_mean":0.895,"score_std":0.015,"stability_score":0.97,"decision":"keep"}
{"sample_id":"b","n_observations":2,"majority_label":"win","label_agreement":0.5,"score_std":0.31,"stability_score":0.42,"decision":"review"}
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

### components フィールドで理由を確認できる

各サブスコアは `StabilityReport.components` に含まれるため、なぜそのスコアになったかを直接確認できます。

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

`--weight-*` フラグで重要な次元を強調できます。

```bash
# LLM judge: 評価者・モデル間の合意を重視
quietset score input.jsonl --weight-labels 1.0 --weight-evaluators 2.0 --weight-models 2.0

# ゲーム探索・シミュレーション: シード・budget への頑健性を重視
quietset score input.jsonl --weight-seed 2.0 --weight-budget 2.0
```

## 判定（decision）

| 条件 | 判定 |
|------|------|
| `stability_score >= 0.85` | `keep`（採用） |
| `stability_score <= 0.40` | `drop`（除外） |
| それ以外 | `review`（要確認） |

`--keep-threshold` と `--drop-threshold` で閾値を変更できます。

## summary コマンド

`quietset summary scored.jsonl` はデータセット全体の診断情報を表示します。

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
// observations は sample_id でソート済みであること
for obs in observations {
    if let Some(report) = scorer.push(obs) {
        println!("{:?}", report.decision);
    }
}
if let Some(report) = scorer.flush() {
    println!("{:?}", report.decision);
}
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

result = quietset.score_jsonl(
    '{"sample_id":"a","label":"win","score":0.9}\n'
    '{"sample_id":"a","label":"win","score":0.88}\n'
)
print(result)
```

## 類似ツールとの比較

| ツール | 何をするか | quietset との違い |
|--------|-----------|------------------|
| **Cleanlab** | 訓練済み分類器と Confident Learning でラベルエラーを検出。分類・回帰・NLP に対応。 | quietset はモデル訓練不要で、タスク固有の前提を持たない。推定ラベル品質ではなく、繰り返し実行をまたいだ安定性でフィルタリングする。 |
| **Label Studio** | 画像・テキスト・音声・時系列のラベリング用 Web アノテーションプラットフォーム。複数アノテーター管理に対応。 | quietset は CLI/ライブラリのプリミティブであってアノテーション UI ではない。他ツールが生成したラベルを受け取り、その安定性を計測する。 |
| **pandas / polars** | 汎用データ操作ライブラリ。std・groupby・集計処理が可能。 | quietset は目的特化の安定性スキーマを提供する。`keep / review / drop` の判定・次元別サブスコア・不安定要因の診断を pandas で自前実装するには相当な手間がかかる。 |
| **Great Expectations / Soda** | null・範囲・スキーマなどのルールに対してデータを検証するデータ品質フレームワーク。 | これらは「データがスキーマに適合しているか」を検査する。quietset は「繰り返し評価をまたいでラベル・スコアが一致しているか」を検査する。関心事は直交している。 |
| **scipy.stats / sklearn metrics** | Cohen's kappa・Fleiss' kappa・評価者間一致などの統計関数。 | quietset は同様のアイデアを、JSONL I/O・サンプル単位レポート・設定可能な閾値を持つコンポーザブルなパイプラインプリミティブとして提供する。scipy で再実装できるが、グルーピング・正規化・重み付け・出力整形を自前で組む必要がある。 |
| **LLM 評価フレームワーク（RAGAS、DeepEval）** | モデルベースの judge を使って LLM 出力を参照回答と照合・スコアリングするフレームワーク。 | quietset は judge に依存しない。judge が生成したスコア・ラベルを受け取り、実行・budget・モデル・シードをまたいだ合意度を計測する。特定の LLM judge を置き換えるのではなく、どの judge とも組み合わせて使える。 |

## ライセンス

MIT OR Apache-2.0
