# quietset

quietset は、タスク固有の前提条件に依存せず、ラベルの安定性によってデータセットをフィルタリングします。

複数の評価者・計算コスト・ランダムシード・モデルチェックポイント・繰り返し実行を通じて、ラベルやスコアが安定しているサンプルだけを残すために使います。

ノイズの多い教師データの整理、合成データのフィルタリング、強化学習のサンプル選択、探索ベースのラベリング、シミュレーション結果の絞り込み、ベンチマークのキュレーションに役立ちます。

quietset はモデルの訓練ツールでも、アノテーションプラットフォームでも、画像品質チェッカーでもありません。他のツールと組み合わせて使う、小さな安定性フィルタリングのプリミティブです。

## インストール

```bash
cargo install --path crates/quietset-cli
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

# CSV 出力
quietset score input.jsonl --output-format csv > scored.csv

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

利用可能なサブスコアの平均として計算されます。

| サブスコア | 意味 |
|-----------|------|
| `label_agreement` | 多数派ラベルを持つ観測の割合 |
| `1 - normalized_score_std` | スコアのばらつきの少なさ |
| `1 - budget_sensitivity` | 計算コスト変化への頑健性 |
| `model_agreement` | モデル間のラベル一致度 |
| `evaluator_agreement` | 評価者間のラベル一致度 |

対応するフィールドがない次元（例：ラベルなし、budget なし）は平均から除外されます。観測が1件のみのサンプルは `stability_score = 0.5`（デフォルトで `review`）になります。

## 判定（decision）

| 条件 | 判定 |
|------|------|
| `stability_score >= 0.85` | `keep`（採用） |
| `stability_score <= 0.40` | `drop`（除外） |
| それ以外 | `review`（要確認） |

`--keep-threshold` と `--drop-threshold` で閾値を変更できます。

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

| ツール | 違い |
|--------|------|
| **Cleanlab** | Python・タスク固有・訓練済み分類器でラベルエラーを検出。quietset はモデル非依存で訓練不要。 |
| **Label Studio** | アノテーション UI。quietset は CLI/ライブラリのプリミティブ。 |
| **pandas** | 汎用データツール。quietset は安定性メトリクスに特化。 |

## ライセンス

MIT OR Apache-2.0
