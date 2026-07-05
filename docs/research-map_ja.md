# 研究マップ：将来の truth model 候補

これはロードマップの確約ではなく、メモです。将来の contributor（あるいは Claude 自身）が
「quietset に X を追加すべきか」をゼロから再検討しなくて済むように、以下の候補は既に検討され、
明示的な理由とともに**今回は追加しない**と判断されたことを記録しています。ここに書かれた
ものは何もスケジュールされていません — それぞれは、具体的に要望が出たときにだけ再検討する
候補です（`tasks/lessons.md` で既に held-out calibration や EV-based active-review に対して
使われている「deferred は永久の却下ではない」というパターンと同じで、deferred の状態は
誰かが明示的に名指しで要望したときに変わるのであって、時間経過で自動的に変わるものでは
ありません）。

## 参考: すでに実装済みのもの

- **Dawid-Skene EM**（`--decision-score latent-truth`、
  `crates/quietset/src/latent_truth.rs`）— 評価者ごとの混同行列による信頼性推定とラベル
  事後分布、`gold_label` は不要。詳細は
  [metrics_ja.md](metrics_ja.md#latent_truth_dawid-skene-em) を参照。

## 候補（未実装）

- **MACE**（Multi-Annotator Competence Estimation）— 各アノテータの competence と潜在的な
  真のラベルを同時に推定する。一部のアノテータが単なるノイズではなく spammer/adversarial
  である可能性を明示的にモデル化する。
- **GLAD**（Generative model of Labels, Abilities, and Difficulties）— Dawid-Skene 系の推定を
  拡張し、評価者ごとの信頼性だけでなくアイテムごとの difficulty パラメータも扱う。
- **Snorkel label model** — 正解ラベルなしで複数のノイズを含む「labeling function」を統合する
  weak supervision の手法。未知の精度や、ソース間の*相関*を明示的にモデル化する。
- **Bayesian annotator model**（より広い枠組み）— 評価者信頼性を点推定ではなく完全な
  事後分布・不確実性として扱う。

### なぜ今はやらないか

Dawid-Skene の独立性の仮定が破れることが分かっていて、かつ上記のいずれかがそれを具体的に
解決する、という実例がまだありません。これらを先回りして追加することは、`AGENTS.md` の
non-goals がまさに警告している種類の過剰な抽象化（誰もまだ直面していないユースケースの
ための設定サーフェス・依存関係）そのものです — `tasks/lessons.md` にも繰り返し出てくる
テーマですが、投機的な汎用性は具体的な理由が出るまでは却下する、という方針です。

### 再検討するための条件

以下**すべて**を満たすこと（どれか一つではない）:

- 重い依存関係が不要であること — `latent_truth.rs` が新しい crate なしに手書きで実装されて
  いるのと同じように、少量の純粋な Rust で実装できること。
- Dawid-Skene の「相関する評価者」という盲点と同じくらい平易に failure mode を説明できること。
- 合成データによるテストで、多数決や現在の `latent-truth` に対する具体的な勝ち筋を示せる
  こと — 「より洗練されている」ではなく、構築可能なフィクスチャでの実際の勝利であること。
- CLI を設定用DSLに肥大化させないこと — `GoldWeighted` と `LatentTruth` が既にそうしてきた
  のと同じ形（新しい `DecisionScore` バリアント + フラグ）に収まること。新しい抽象化層は
  作らない（`--decision-score` の上に `--truth-model` セレクタを重ねる案は今回検討されたが、
  既存の enum がすでにきれいに拡張できることが `GoldWeighted` と `LatentTruth` で2回証明
  されているため、この理由で見送られた）。

## このリストに明示的に含まれないもの

quietset の*デフォルト* `--decision-score` を `raw` から変更すること、および
`latent_truth_confidence`/`weighted_label_confidence` に対する EM 信頼度のキャリブレーション
層（Platt scaling / isotonic regression / temperature scaling）— どちらも議論の中では
出たものの、具体的な要望になる前に見送られました。もしどちらかが再度提案された場合は、
このドキュメントが事前承認したものとしてではなく、明示的な確認が必要な新しいデフォルト
変更の判断として扱ってください（この種の判断をこのプロジェクトが慎重に扱う理由については
`tasks/lessons.md` の「score_sign が1ではなく0をデフォルトにした理由」と「game-ai プロファイル
のデフォルト変更」のエントリを参照）。
