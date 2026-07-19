# 指標リファレンス

quietset が出力するすべての数値について、数式・出典・解釈・前提・failure mode・quietset での使い方を詳しくまとめたリファレンスです。どの指標が既存の統計量でどれが quietset 独自のヒューリスティックかというコンパクトな分類表は [README_ja.md#これらの数値の根拠](../README_ja.md#これらの数値の根拠) を参照してください。このファイルはそのテーブルが指し示す詳細版です — 指標を変更するときは両方更新してください。ただし数式・前提・failure mode の詳しさはこちら側に置き、テーブル側では重複させないこと。

## label_agreement_lcb

- **数式**: `label_agreement` の Wilson score interval 下側信頼限界（`crates/quietset/src/scoring.rs:23-43`）。
- **出典**: Wilson score interval（Wilson, 1927）。二項比率のための信頼区間。
- **解釈**: `n` 件の観測に基づく、真のラベル一致率の保守的な下限。`n` が増えるほど生の `label_agreement` に近づく。
- **前提**: 繰り返し観測を、多数派ラベルを支持する独立なベルヌーイ試行として扱う。
- **failure mode**: 区間の `z` 分位点は正規分布の逆関数の有理近似から得ており、厳密な分位点関数ではない（実用上は無視できる差）。より重要な点として、Wilson は小標本の**ノイズ**を補正するものであり、評価者間の**相関**を補正するものではない — 評価者が系統的なバイアスを共有していれば、Wilson が見る前の比率そのものが既に水増しされているため、LCB が高いことは相関バイアスに対する防御にはならない。
- **quietset での使い方**: `--decision-score lcb`。`label_agreement_lcb` は、実際の accept/reject 判断において生の `stability_score` より優先すべき「既存の統計量」候補の一つ。

## fleiss_kappa

- **数式**: サブジェクトごとの観測一致率 `P_i` を平均した `P̄`、周辺カテゴリ比率から求めた偶然一致率 `P_e`、`κ = (P̄ - P_e) / (1 - P_e)`（`crates/quietset/src/agreement.rs:8-55`）。
- **出典**: Fleiss' kappa（Fleiss, 1971）。
- **解釈**: `0` = 偶然レベルの一致、`1` = 完全一致、負の値 = 偶然より悪い一致。
- **前提**: 名義尺度（順序のないカテゴリ）。この実装は、古典的な「サブジェクトごとに評価者数固定」の式を、サブジェクトごとに評価者数が可変な場合に一般化している（評価が2件未満のサブジェクトは除外） — 標準的で妥当な拡張だが、評価者数が全サブジェクトで揃っていることを前提とするツールとは数値が完全には一致しない。
- **failure mode**: 偶然一致のみを補正しており、評価者の能力差は補正しない — 全員が同じ間違え方をしている場合でも kappa=1.0（完全一致）になり得る。
- **quietset での使い方**: `reliability` コマンド、データセット全体の診断（末尾のJSONL行）。

## krippendorff_alpha

- **数式**: coincidence matrix に基づく導出 — `Do = 1 - e_diag/n`、`De = (n² - Σn_c²) / (n(n-1))`、`α = 1 - Do/De`（`crates/quietset/src/agreement.rs:62-109`）。
- **出典**: Krippendorff's alpha。
- **解釈**: kappa よりも一般的な信頼性係数で、スケールは同じ。欠損データや評価者数のばらつきにより頑健。
- **前提**: **nominal**（名義尺度）の距離関数のみが実装されている — ordinal/interval/ratio のバリアントはこのコードベースには存在しない。
- **failure mode**: 「nominal」という限定を付けずに「Krippendorff's alpha」と説明すると、実装されている範囲を過大に見せてしまう。順序付きラベルや数値ラベルの距離には対応していない。
- **quietset での使い方**: `reliability` コマンド、`fleiss_kappa` と並んで出力。

## label_entropy

- **数式**: ラベル分布に対する Shannon entropy `-Σp·log2(p)` を、`log2(そのサンプルで観測された distinct ラベル数)` で正規化したもの（`crates/quietset/src/scoring.rs:135-145`）。
- **出典**: Shannon entropy。
- **解釈**: `0.0` = 満場一致、`1.0` = そのサンプルが受け取ったラベルの中で一様に割れている。
- **前提/failure mode**: 正規化の分母は「そのサンプルで観測されたラベル数」であり、ラベル空間全体のサイズではない。2種類のラベルしか観測されなかったサンプルも5種類観測されたサンプルも同じく `1.0` に達し得るため、distinct ラベル数が異なるサンプル間で生の値を単純比較することはできない。
- **quietset での使い方**: 診断フィールド。`active-review` の `high_entropy` シグナルに使われる。

## score_std / score_mad / score_iqr

- **数式**: `score_std` = 母標準偏差（`n` で割る、Bessel補正なし）、`score_mad` = 中央値絶対偏差（`|s - 中央値|` の中央値）、`score_iqr` = 順序統計量の線形補間による `Q3 - Q1`（NumPy/R のデフォルトの分位点計算方法と一致）（`crates/quietset/src/scoring.rs:167-190`）。
- **出典**: 標準的な分散指標。
- **解釈**: サンプル内の数値スコアの散らばり具合。`score_consistency` コンポーネントの元になる。
- **前提/failure mode**: `score_std` は Bessel 補正がないため、`n` が小さいとき（目安として10件未満）真の分散をわずかに過小評価する。MAD/IQR は設計上外れ値に頑健な反面、小さく一貫したシフトへの感度は下がる。
- **quietset での使い方**: `--score-dispersion std|mad|iqr` で `score_consistency` に使う指標を選択。

## latent_truth_*（Dawid-Skene EM）

- **数式**: 評価者ごとの混同行列とラベル事前分布に対する EM（期待値最大化）。Laplace smoothing、log-space での E-step、20回で打ち切り（`crates/quietset/src/latent_truth.rs:54-381`）。
- **出典**: Dawid & Skene (1979) — 正解ラベルが直接観測できない状況で、潜在的な真のラベルと評価者ごとの誤り率を推定する古典的手法。
- **解釈**: `latent_truth_label`/`latent_truth_confidence`/`latent_truth_label_distribution` — `gold_label` を一切必要とせず、不一致パターンのみから推定した信頼性重み付き投票。`latent_truth_converged`/`latent_truth_iterations`/`latent_truth_convergence_delta` は、EM が実際に収束したのか、それとも iteration 上限に達しただけなのかを開示する。
- **前提**: 評価者の誤りは、真のラベルが与えられた条件下で独立である — Dawid-Skene の中心的な仮定。
- **failure mode**: 評価者の誤りが相関しているとこの独立性の仮定が崩れ、自信満々に間違った事後分布を出すことがある — `latent_truth_confidence` は EM 自身が当てはめた事後確率であり、外部からの正しさのチェックではない。`evaluator_effective_n`（相関のある評価者ペアに対する phi 係数を用いた Kish design effect）と `correlated_evaluator_warning` は、**収束した EM の出力の上に quietset が独自に追加した診断指標**であり、オリジナルの Dawid-Skene 論文の一部ではない — *他の*評価者がその相関ブロックの偏りを暴露した場合にしか検出できず、相関ブロックが EM の consensus そのものになってしまった場合は警告が出ない（既知の盲点として文書化されている）。
- **quietset での使い方**: `--decision-score latent-truth`。安全側デモーション用フラグ（`--demote-on-correlated-warning`、`--min-evaluator-effective-n <f64>`、`--demote-on-non-convergence`）は、これらの診断が発火したときに Keep から Review へ格下げする（Drop には絶対に落とさない） — これらは EM 自身のフィットに関する不確実性シグナルであり、ラベル自体が間違っているという証拠ではないため（このロジックのコード上の該当箇所は `crates/quietset/src/lib.rs` の demotion 適用部分を参照）。

## stable_wrong_rate_among_keep

- **数式**: `Keep` 判定サンプルのうち、多数決/重み付き/latent ラベルが `gold_label` と食い違う割合。
- **出典**: なし — モデルによる推定ではなく直接の経験的計測。
- **解釈**: 「一見安定して見えるデータ」が gold に照らして実際にどれくらい間違っているか。
- **前提**: `gold_label` が必要。入力自体が自分で管理している held-out split でない限り、in-sample での計測になる。
- **failure mode**: `gold-weighted` decision-score バリアントはここで循環的になる — 評価者ごとの信頼性重みが、その後グレーディングに使われるのと同じ gold ラベルに対してフィットされるため、`stable_wrong_rate` が人為的に低く出ることがある（`calibrate`/`policy` と共通する循環構造の全体像は `tasks/lessons.md` を参照）。
- **quietset での使い方**: `stable-wrong-risk` コマンド。`--breakdown` で特定の evaluator/model/budget にこの率を帰属させる。

## stability_score

- **数式**: そのサンプルで利用可能なサブスコア（`label`、`score_consistency`、`score_sign_agreement`、`budget_robustness`、`seed_robustness`、`model_agreement`、`evaluator_agreement`）の加重平均。欠けている次元は分子・分母の両方から除外される（`crates/quietset/src/scoring.rs:225-280`）。
- **出典**: なし — quietset 独自の合成指標。
- **解釈**: 単一の `[0,1]` のランキング・確認用スコアであり、何かの確率ではない。
- **前提**: サブスコア同士が線形平均するのに十分互換であること。観測が1件だけの場合は外挿された極端な値ではなく固定の `0.5`（デフォルトで review）になる。
- **failure mode**: **キャリブレーションされた確率ではなく、正しさの保証でもない** — 全評価者が一貫して間違った答えに合意しているサンプルは `1.0` になる。`label_agreement_lcb` と違い、それ自体に不確実性モデルを持たない。
- **quietset での使い方**: `filter`/閾値判定が作用するデフォルトのスコア。実際の accept/reject の判断には、生の `stability_score` ではなく `label_agreement_lcb`、`latent-truth`、`gold-weighted`、または `calibrate --heldout` で選んだ閾値を優先してください — [README_ja.md#安定性スコアstability_score](../README_ja.md#安定性スコアstability_score) を参照。

## confidence / adjusted_stability_score

- **数式**: `confidence = n / (n + k)`（`k` のデフォルトは `3.0`）、`adjusted_stability_score = stability_score * confidence + 0.5 * (1 - confidence)`（`crates/quietset/src/scoring.rs:282-283`）。
- **出典**: なし — Bayesian shrinkage に着想を得た quietset 独自のヒューリスティックであり、名前の付いた引用可能な推定量ではない。
- **解釈**: 観測数が少ないのに高い `stability_score` を、中立値 `0.5` へ引き寄せる。
- **前提**: `k = 3.0` はチューニングされたデフォルト値であり、特定の目標誤検出率から導出されたものではない。
- **failure mode**: Wilson の LCB と異なり、`stability_score` の実際のサンプリング分布をモデル化していない — 元の統計量がどれだけ極端か・素直かに関わらず、同じ `n` であれば同じだけ shrink される。
- **quietset での使い方**: `--decision-score adjusted` / `--use-adjusted-score`。

## score_sign_agreement

- **数式**: `max(負の個数, ゼロの個数, 正の個数) / 総数`（`crates/quietset/src/scoring.rs:192-206`）。
- **出典**: なし — 符号付き・ゼロ中心のスコア（将棋・チェス型の評価値）向けのドメイン固有ヒューリスティック。
- **解釈**: magnitude ベースの分散指標（`score_std`/`score_mad`/`score_iqr`）を補完する — `[+0.01, -0.01]` は magnitude だけ見るとほぼ完全に安定だが、符号は五分五分になる。
- **failure mode**: ゼロ値のスコアは独立した第3のバケツを形成し、正負どちらにも統合されない — 厳密なゼロが多いバッチでは「ゼロ」自体が多数派の符号になり得る。
- **quietset での使い方**: 常に計算されるが、デフォルトでは `stability_score` から除外される（`--weight-score-sign 0`）。`--profile game-ai` はデフォルトで `×2` の重みを付ける。

## 軌道安定性指標（gradient_sign_agreement、update_direction_agreement、teacher_residual_stability、shuffle_seed_sensitivity、loss_recipe_agreement）

- **数式**: いずれも既存の汎用関数をそのまま別のフィールドに適用したもの — `gradient_sign_agreement`/`update_direction_agreement`は`score_sign_agreement`が使う`sign_agreement`ヘルパーをそのまま再利用、`teacher_residual_stability`は`score_consistency`と同じ形の`1 - normalized_std(teacher_residual)`、`shuffle_seed_sensitivity`は`compute_range_sensitivity`を`seed`ではなく`shuffle_seed`でキーして再利用、`loss_recipe_agreement`は`compute_group_label_agreement`を`model_id`/`evaluator_id`ではなく`loss_recipe`でキーして再利用（`crates/quietset/src/scoring.rs`）。
- **出典**: なし — ハーネス側が供給する軌道信号向けのドメイン非依存ヒューリスティック。`score_sign_agreement`と同じ位置づけ。
- **解釈**: ハーネス側が事前計算した学習軌道信号のcross-run一貫性（quietset自身は勾配やcheckpointに一切触れない — なぜこれがquietsetのnon-goalsの範囲外にとどまるかは`docs/research-map.md`参照）。対応する`Observation`フィールドが無ければ`None`。
- **前提/failure mode**: それぞれが再利用する指標と同じ — `sign_agreement`はゼロ値を独立した第3のバケツとして扱う（上記`score_sign_agreement`参照）。`shuffle_seed_sensitivity`は`seed_sensitivity`と同じく、異なる`shuffle_seed`が2つ以上必要。
- **quietset での使い方**: 対応するフィールドが存在すれば常に計算されるが、`score_sign`と全く同様に`stability_score`からはデフォルトで除外される（`ScoreWeights::{gradient_sign,update_cosine,teacher_residual,shuffle_seed,loss_recipe}`はすべてデフォルト`0.0`）——`--weight-gradient-sign`/`--weight-update-cosine`/`--weight-teacher-residual`/`--weight-shuffle-seed`/`--weight-loss-recipe`で有効化。

## dead_unit_rate / saturated_unit_rate / trajectory_effect_mean / trajectory_effect_sign_agreement

- **数式**: `dead_unit_rate`/`saturated_unit_rate` = `dead_unit_count`/`saturated_unit_count`を持つ観測のうち値が`0.0`より大きいものの割合。`trajectory_effect_mean`/`trajectory_effect_sign_agreement` = `trajectory_effect`の平均値と`sign_agreement`（`crates/quietset/src/scoring.rs`）。
- **出典**: なし — 診断専用フィールドで、`stability_score`の一部ではない。
- **解釈**: あるサンプルがどれだけ頻繁に新規のdead/saturatedユニットを誘発するか、また正味の学習効果（例: radial normの変化量）が成長方向か縮小方向か。これらは`block-score`の分類（pathologicalの上書き、成長/縮小の符号）に使われ、サンプル単位の安定性には使われない。
- **quietset での使い方**: `block-score`/`trajectory-audit`（下記参照）。これらに対応する`ScoreWeights`フィールドは存在せず、`stability_score`には一切入らない。

## keep / drop 閾値（0.85 / 0.40）

- **数式**: なし — `Thresholds::default()`（`crates/quietset/src/decision.rs:11-18`）。
- **出典**: なし — ユーザーが上書き可能な、単なる運用上のデフォルト値。
- **解釈**: 確信を持って keep する範囲と確信を持って drop する範囲の間に、意図的な review 帯を残す。
- **前提/failure mode**: 特定の目標精度や誤り率に対してキャリブレーションされたものではない — 妥当な汎用デフォルトであり、導出された定数ではない。計測された精度保証付きの閾値が必要なら、生のデフォルト値ではなく `calibrate`（できれば `--heldout` 付き）を使ってください。
- **quietset での使い方**: `score` などのコマンドの `--keep-threshold`/`--drop-threshold`。

## calibrate と policy は selective classification である

`calibrate`/`policy` は、概念的には *selective classification* / *reject option* / risk-coverage のトレードオフに近いです：すべてのサンプルを無理やりどちらかに分類するのではなく、不確かなものを `review`（abstain）に回すことで、実際に採用する範囲のリスクを制御します。この整理では: `keep` = accept、`review` = abstain/defer、`drop` = 学習用途としての reject、となります。

ただし、これは **conformal prediction のような厳密な保証ではありません**。閾値は1つの gold-labeled サンプルに対するグリッドサーチによって経験的に選ばれます。同じサンプルに対する `achieved_precision` は、その閾値がまさにそのサンプルに対して選ばれたものであるため楽観的です（in-sample-optimism を示す `note` フィールドと `--heldout` の説明は [README_ja.md#calibrate-コマンド](../README_ja.md#calibrate-コマンド) を参照）。`achieved_precision` は「証明された値」ではなく「計測された値」として扱ってください — `--heldout` を使えばより誠実な、しかしそれでもなお経験的な推定が得られます。
