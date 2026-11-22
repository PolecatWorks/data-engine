//! Built-in, AST-validated, SHA-256-pinned scientific Python algorithms for Spec 10 & Spec 11.
//! Every script strictly uses only allowlisted modules (`math`, `statistics`, `json`, `numpy`,
//! `scipy`, `statsmodels`, `pyarrow`) and defines top-level `def run(inputs, params):`.

pub const BUILTIN_STAT_DESCRIBE_CODE: &str = r#"
import math
import numpy as np
from scipy import stats

def _downsample_pairs(xs, ys, max_pts):
    n = len(xs)
    if n <= max_pts:
        return [[round(float(a), 6), round(float(b), 6)] for a, b in zip(xs, ys)]
    idx = np.linspace(0, n - 1, max_pts, dtype=int)
    return [[round(float(xs[i]), 6), round(float(ys[i]), 6)] for i in idx]

def run(inputs, params):
    tbl = inputs["data"]
    columns = params.get("columns", [])
    dataset_name = params.get("dataset_name", "dataset")
    max_pts = int(params.get("max_plot_points", 1000))

    col_stats = {}
    assumption_checks = []
    diagnostic_plots = []
    learned_field_traits = []

    for col in columns:
        arr_py = tbl.column(col).to_pylist()
        total_n = len(arr_py)
        clean = [float(v) for v in arr_py if v is not None and not (isinstance(v, float) and math.isnan(v))]
        n = len(clean)
        null_count = total_n - n
        null_ratio = float(null_count) / float(total_n) if total_n > 0 else 0.0

        if n == 0:
            col_stats[col] = {"count": 0, "null_count": null_count, "null_ratio": null_ratio}
            assumption_checks.append({
                "check": f"sample_size_{col}",
                "status": "failed",
                "statistic": 0.0,
                "p_value": None,
                "message": f"Column '{col}' has zero non-null numeric observations."
            })
            continue

        x = np.asarray(clean, dtype=np.float64)
        mean_val = float(np.mean(x))
        std_val = float(np.std(x, ddof=1)) if n > 1 else 0.0
        var_val = float(np.var(x, ddof=1)) if n > 1 else 0.0
        skew_val = float(stats.skew(x, bias=False)) if n > 2 and std_val > 0.0 else 0.0
        kurt_val = float(stats.kurtosis(x, fisher=True, bias=False)) if n > 3 and std_val > 0.0 else 0.0

        q_probs = [0.01, 0.05, 0.25, 0.50, 0.75, 0.95, 0.99]
        q_vals = [float(v) for v in np.quantile(x, q_probs)]
        q_map = {
            "p1": q_vals[0], "p5": q_vals[1], "p25": q_vals[2],
            "p50": q_vals[3], "p75": q_vals[4], "p95": q_vals[5], "p99": q_vals[6],
        }
        iqr = q_map["p75"] - q_map["p25"]
        med = q_map["p50"]
        mad = float(np.median(np.abs(x - med)))

        lower_fence = q_map["p25"] - 1.5 * iqr
        upper_fence = q_map["p75"] + 1.5 * iqr
        iqr_outliers = int(np.sum((x < lower_fence) | (x > upper_fence)))
        if mad > 0.0:
            mod_z = 0.6745 * (x - med) / mad
            mod_z_outliers = int(np.sum(np.abs(mod_z) > 3.5))
        else:
            mod_z_outliers = 0

        if std_val == 0.0:
            norm_stat, norm_p, norm_method = 0.0, 0.0, "zero_variance"
            norm_status = "failed"
        elif n <= 5000 and n >= 3:
            res = stats.shapiro(x)
            norm_stat, norm_p, norm_method = float(res.statistic), float(res.pvalue), "shapiro_wilk"
            norm_status = "passed" if norm_p >= 0.05 else "failed"
        elif n > 5000:
            res = stats.normaltest(x)
            norm_stat, norm_p, norm_method = float(res.statistic), float(res.pvalue), "dagostino_pearson"
            norm_status = "passed" if norm_p >= 0.05 else "failed"
        else:
            norm_stat, norm_p, norm_method = 0.0, 1.0, "insufficient_n"
            norm_status = "warning"

        is_heavy_tailed = bool(abs(skew_val) > 1.5 or kurt_val > 3.0 or (n > 0 and mod_z_outliers / float(n) > 0.01))
        if std_val == 0.0:
            dist_family = "constant"
            rec_family = "none"
        elif is_heavy_tailed and float(np.min(x)) >= 0.0 and skew_val > 1.0:
            dist_family = "log_normal_heavy_tailed"
            rec_family = "non_parametric_or_log_transform"
        elif norm_status == "passed":
            dist_family = "normal"
            rec_family = "parametric_welch_t"
        else:
            dist_family = "non_normal"
            rec_family = "non_parametric"

        col_stats[col] = {
            "count": n,
            "null_count": null_count,
            "null_ratio": round(null_ratio, 6),
            "mean": mean_val,
            "std": std_val,
            "variance": var_val,
            "skewness": skew_val,
            "excess_kurtosis": kurt_val,
            "quantiles": q_map,
            "iqr": iqr,
            "mad": mad,
            "iqr_outlier_count": iqr_outliers,
            "modified_z_outlier_count": mod_z_outliers,
            "normality_test": {"method": norm_method, "statistic": norm_stat, "p_value": norm_p},
            "is_heavy_tailed": is_heavy_tailed,
            "distribution_family": dist_family,
        }

        assumption_checks.append({
            "check": f"normality_{col}",
            "status": norm_status,
            "statistic": round(norm_stat, 6),
            "p_value": round(norm_p, 6),
            "message": f"{norm_method} for '{col}': p={norm_p:.4g}, skew={skew_val:.3f}, excess_kurt={kurt_val:.3f}, outliers(mod_z)={mod_z_outliers}."
        })

        # Histogram & Q-Q plot bounded to max_pts
        bins = min(30, max(5, int(math.sqrt(n))))
        counts, edges = np.histogram(x, bins=bins)
        diagnostic_plots.append({
            "plot_type": "distribution_histogram",
            "title": f"Distribution of {col}",
            "series": [{
                "name": col,
                "bins": [round(float(e), 6) for e in edges[:max_pts]],
                "counts": [int(c) for c in counts[:max_pts]],
            }]
        })

        sorted_x = np.sort(x)
        probs = (np.arange(1, n + 1) - 0.5) / float(n)
        theo_q = stats.norm.ppf(probs)
        diagnostic_plots.append({
            "plot_type": "qq_plot",
            "title": f"Normal Q-Q Plot ({col})",
            "series": [{
                "name": col,
                "points": _downsample_pairs(theo_q, sorted_x, max_pts),
            }]
        })

        learned_field_traits.append({
            "field_canonical_id": f"{dataset_name}.{col}",
            "distribution_family": dist_family,
            "is_heavy_tailed": is_heavy_tailed,
            "skewness": round(skew_val, 4),
            "kurtosis": round(kurt_val, 4),
            "seasonality_period": None,
            "recommended_test_family": rec_family,
            "known_confounders": [],
            "epistemic_status": "empirically_tested",
            "confidence": 0.92 if n >= 30 else 0.70,
        })

    return {
        "primary_result": {"columns": col_stats},
        "assumption_checks": assumption_checks,
        "recommended_alternative": None,
        "diagnostic_plots": diagnostic_plots,
        "learned_field_traits": learned_field_traits,
        "group_sizes": {},
    }
"#;

pub const BUILTIN_STAT_HYPOTHESIS_CODE: &str = r#"
import math
import numpy as np
from scipy import stats
from statsmodels.stats.multitest import multipletests

def _cohens_d_and_hedges_g(a, b):
    na, nb = len(a), len(b)
    if na < 2 or nb < 2:
        return 0.0, 0.0
    va = float(np.var(a, ddof=1))
    vb = float(np.var(b, ddof=1))
    pooled = math.sqrt(((na - 1) * va + (nb - 1) * vb) / float(na + nb - 2))
    if pooled == 0.0:
        return 0.0, 0.0
    d = (float(np.mean(a)) - float(np.mean(b))) / pooled
    j = 1.0 - (3.0 / (4.0 * (na + nb - 2) - 1.0))
    return d, d * j

def _cliffs_delta(a, b):
    na, nb = len(a), len(b)
    if na == 0 or nb == 0:
        return 0.0
    if na * nb <= 200000:
        diff = a[:, None] - b[None, :]
        gt = int(np.sum(diff > 0))
        lt = int(np.sum(diff < 0))
        return float(gt - lt) / float(na * nb)
    u_stat, _ = stats.mannwhitneyu(a, b, alternative="two-sided")
    return float(2.0 * u_stat / float(na * nb) - 1.0)

def _interpret_d(val):
    av = abs(val)
    if av < 0.2:
        return "negligible"
    if av < 0.5:
        return "small"
    if av < 0.8:
        return "medium"
    return "large"

def _welch_ci(a, b, alpha):
    na, nb = len(a), len(b)
    diff = float(np.mean(a) - np.mean(b))
    va = float(np.var(a, ddof=1)) if na > 1 else 0.0
    vb = float(np.var(b, ddof=1)) if nb > 1 else 0.0
    se = math.sqrt(va / float(na) + vb / float(nb))
    if se == 0.0:
        return diff, diff
    num = (va / na + vb / nb) ** 2
    den = ((va / na) ** 2) / max(1, na - 1) + ((vb / nb) ** 2) / max(1, nb - 1)
    df = num / den if den > 0.0 else float(na + nb - 2)
    t_crit = float(stats.t.ppf(1.0 - alpha / 2.0, df))
    return diff - t_crit * se, diff + t_crit * se

def _normality_check(arr, label):
    n = len(arr)
    if n < 3:
        return {"check": f"normality_{label}", "status": "warning", "statistic": 0.0, "p_value": 1.0, "message": "n < 3"}
    if float(np.std(arr, ddof=1)) == 0.0:
        return {"check": f"normality_{label}", "status": "failed", "statistic": 0.0, "p_value": 0.0, "message": "zero variance"}
    if n <= 5000:
        res = stats.shapiro(arr)
        stat, p = float(res.statistic), float(res.pvalue)
        meth = "shapiro_wilk"
    else:
        res = stats.normaltest(arr)
        stat, p = float(res.statistic), float(res.pvalue)
        meth = "dagostino_pearson"
    skew = float(stats.skew(arr, bias=False))
    status = "passed" if (p >= 0.05 and abs(skew) <= 1.5) else "failed"
    return {
        "check": f"normality_{label}",
        "status": status,
        "statistic": round(stat, 6),
        "p_value": round(p, 6),
        "message": f"{meth} on '{label}': p={p:.4g}, skew={skew:.3f}"
    }

def _run_two_sample(method, a, b, alpha, alt):
    if method == "student_t":
        res = stats.ttest_ind(a, b, equal_var=True, alternative=alt)
        stat, p = float(res.statistic), float(res.pvalue)
        d, g = _cohens_d_and_hedges_g(a, b)
        lo, hi = _welch_ci(a, b, alpha)
        eff = {"metric": "hedges_g", "value": round(g, 6), "interpretation": _interpret_d(g)}
    elif method == "mann_whitney_u":
        res = stats.mannwhitneyu(a, b, alternative=alt)
        stat, p = float(res.statistic), float(res.pvalue)
        cd = _cliffs_delta(a, b)
        med_diff = float(np.median(a) - np.median(b))
        eff = {"metric": "cliffs_delta", "value": round(cd, 6), "interpretation": _interpret_d(cd * 2.0)}
        lo, hi = med_diff, med_diff
    elif method == "paired_t":
        res = stats.ttest_rel(a, b, alternative=alt)
        stat, p = float(res.statistic), float(res.pvalue)
        diff = a - b
        sd = float(np.std(diff, ddof=1))
        d = float(np.mean(diff)) / sd if sd > 0.0 else 0.0
        se = sd / math.sqrt(len(diff))
        t_crit = float(stats.t.ppf(1.0 - alpha / 2.0, max(1, len(diff) - 1)))
        lo, hi = float(np.mean(diff)) - t_crit * se, float(np.mean(diff)) + t_crit * se
        eff = {"metric": "cohens_dz", "value": round(d, 6), "interpretation": _interpret_d(d)}
    elif method == "wilcoxon":
        res = stats.wilcoxon(a - b, alternative=alt)
        stat, p = float(res.statistic), float(res.pvalue)
        med_diff = float(np.median(a - b))
        eff = {"metric": "median_diff", "value": round(med_diff, 6), "interpretation": "non_parametric"}
        lo, hi = med_diff, med_diff
    else:
        res = stats.ttest_ind(a, b, equal_var=False, alternative=alt)
        stat, p = float(res.statistic), float(res.pvalue)
        d, g = _cohens_d_and_hedges_g(a, b)
        lo, hi = _welch_ci(a, b, alpha)
        eff = {"metric": "hedges_g", "value": round(g, 6), "interpretation": _interpret_d(g)}
    return {
        "method": method,
        "test_statistic": round(stat, 6),
        "p_value": float(p),
        "effect_size": eff,
        "confidence_interval": {"level": 1.0 - alpha, "lower": round(lo, 6), "upper": round(hi, 6)},
        "mean_a": float(np.mean(a)),
        "mean_b": float(np.mean(b)),
        "median_a": float(np.median(a)),
        "median_b": float(np.median(b)),
    }

def run(inputs, params):
    tbl = inputs["data"]
    method = params.get("method", "welch_t")
    value_col = params.get("value_col")
    group_col = params.get("group_col")
    col_a = params.get("col_a")
    col_b = params.get("col_b")
    alpha = float(params.get("alpha", 0.05))
    alt = params.get("alternative", "two-sided")
    correction = params.get("correction", "none")
    segment_col = params.get("segment_col")
    stratify_by = params.get("stratify_by")
    dataset_name = params.get("dataset_name", "dataset")
    max_pts = int(params.get("max_plot_points", 1000))

    assumption_checks = []
    recommended_alternative = None
    diagnostic_plots = []
    learned_field_traits = []
    group_sizes = {}

    # Multi-segment hypothesis testing with FDR / Holm / Bonferroni
    if segment_col and group_col and value_col:
        segs = tbl.column(segment_col).to_pylist()
        grps = tbl.column(group_col).to_pylist()
        vals = tbl.column(value_col).to_pylist()

        by_seg = {}
        for s, g, v in zip(segs, grps, vals):
            if s is None or g is None or v is None:
                continue
            by_seg.setdefault(str(s), {}).setdefault(str(g), []).append(float(v))

        seg_results = []
        raw_p = []
        for s_name in sorted(by_seg.keys()):
            g_keys = sorted(by_seg[s_name].keys())
            if len(g_keys) < 2:
                continue
            a = np.asarray(by_seg[s_name][g_keys[0]], dtype=np.float64)
            b = np.asarray(by_seg[s_name][g_keys[1]], dtype=np.float64)
            r = _run_two_sample(method, a, b, alpha, alt)
            r["segment"] = s_name
            r["groups"] = [g_keys[0], g_keys[1]]
            r["n_a"] = int(len(a))
            r["n_b"] = int(len(b))
            seg_results.append(r)
            raw_p.append(r["p_value"])

        adj_method = None
        if correction and correction != "none" and len(raw_p) > 0:
            mc_map = {"fdr_bh": "fdr_bh", "holm": "holm", "bonferroni": "bonferroni"}
            mc_name = mc_map.get(correction, "fdr_bh")
            reject, p_adj, _, _ = multipletests(raw_p, alpha=alpha, method=mc_name)
            adj_method = mc_name
            for r, rej, pa in zip(seg_results, reject, p_adj):
                r["adjusted_p_value"] = float(pa)
                r["significant_after_adjustment"] = bool(rej)
        else:
            for r in seg_results:
                r["adjusted_p_value"] = None
                r["significant_after_adjustment"] = bool(r["p_value"] < alpha)

        raw_sig = sum(1 for r in seg_results if r["p_value"] < alpha)
        adj_sig = sum(1 for r in seg_results if r["significant_after_adjustment"])

        assumption_checks.append({
            "check": "multiple_comparison_control",
            "status": "passed" if adj_method else ("warning" if len(seg_results) > 1 else "passed"),
            "statistic": float(len(seg_results)),
            "p_value": None,
            "message": f"Tested {len(seg_results)} segments (raw p<{alpha}: {raw_sig}, adjusted significant: {adj_sig}, adjustment={adj_method})."
        })

        return {
            "primary_result": {
                "method": method,
                "segment_count": len(seg_results),
                "adjustment_method": adj_method,
                "raw_significant_count": raw_sig,
                "adjusted_significant_count": adj_sig,
                "segments": seg_results,
            },
            "assumption_checks": assumption_checks,
            "recommended_alternative": None,
            "diagnostic_plots": [{
                "plot_type": "ci_forest",
                "title": f"Per-Segment Effect Sizes ({segment_col})",
                "series": [{
                    "name": r["segment"],
                    "estimate": r["effect_size"]["value"],
                    "lower": r["confidence_interval"]["lower"],
                    "upper": r["confidence_interval"]["upper"],
                    "p_value": r["p_value"],
                    "adjusted_p_value": r["adjusted_p_value"],
                } for r in seg_results[:max_pts]]
            }],
            "learned_field_traits": [],
            "group_sizes": {r["segment"]: r["n_a"] + r["n_b"] for r in seg_results},
        }

    # Paired columns mode
    if method in ("paired_t", "wilcoxon") and col_a and col_b:
        a = np.asarray([float(v) for v in tbl.column(col_a).to_pylist() if v is not None], dtype=np.float64)
        b = np.asarray([float(v) for v in tbl.column(col_b).to_pylist() if v is not None], dtype=np.float64)
        n = min(len(a), len(b))
        a, b = a[:n], b[:n]
        primary = _run_two_sample(method, a, b, alpha, alt)
        norm_c = _normality_check(a - b, "paired_diff")
        assumption_checks.append(norm_c)
        if method == "paired_t" and norm_c["status"] == "failed":
            recommended_alternative = {
                "method": "wilcoxon",
                "reason": "Paired differences failed normality; Wilcoxon signed-rank test computed.",
                "result": _run_two_sample("wilcoxon", a, b, alpha, alt),
            }
        return {
            "primary_result": primary,
            "assumption_checks": assumption_checks,
            "recommended_alternative": recommended_alternative,
            "diagnostic_plots": [],
            "learned_field_traits": [],
            "group_sizes": {col_a: int(len(a)), col_b: int(len(b))},
        }

    # Chi-square / Fisher exact on categorical or binary value_col by group_col
    if method in ("chi_square", "fisher_exact"):
        grps = tbl.column(group_col).to_pylist()
        vals = tbl.column(value_col).to_pylist()
        g_levels = sorted({str(g) for g in grps if g is not None})
        v_levels = sorted({str(v) for v in vals if v is not None})
        table = np.zeros((len(g_levels), len(v_levels)), dtype=np.int64)
        g_idx = {g: i for i, g in enumerate(g_levels)}
        v_idx = {v: i for i, v in enumerate(v_levels)}
        for g, v in zip(grps, vals):
            if g is not None and v is not None:
                table[g_idx[str(g)], v_idx[str(v)]] += 1
        chi2, p_chi, dof, expected = stats.chi2_contingency(table)
        min_exp = float(np.min(expected))
        n_total = int(np.sum(table))
        cramers_v = math.sqrt(chi2 / float(n_total * max(1, min(table.shape) - 1))) if n_total > 0 else 0.0
        exp_ok = min_exp >= 5.0
        assumption_checks.append({
            "check": "chi_square_expected_cell_counts",
            "status": "passed" if exp_ok else "failed",
            "statistic": round(min_exp, 4),
            "p_value": None,
            "message": f"Minimum expected cell count = {min_exp:.2f} (threshold >= 5)."
        })
        if method == "fisher_exact" and table.shape == (2, 2):
            odds, p_f = stats.fisher_exact(table)
            primary = {
                "method": "fisher_exact",
                "test_statistic": float(odds),
                "p_value": float(p_f),
                "effect_size": {"metric": "odds_ratio", "value": float(odds), "interpretation": "exact_2x2"},
                "confidence_interval": None,
            }
        else:
            primary = {
                "method": "chi_square",
                "test_statistic": float(chi2),
                "p_value": float(p_chi),
                "dof": int(dof),
                "effect_size": {"metric": "cramers_v", "value": round(cramers_v, 6), "interpretation": _interpret_d(cramers_v * 2.0)},
                "confidence_interval": None,
            }
            if not exp_ok and table.shape == (2, 2):
                odds, p_f = stats.fisher_exact(table)
                recommended_alternative = {
                    "method": "fisher_exact",
                    "reason": "Expected cell count < 5 in 2x2 table; Fisher's exact test computed.",
                    "result": {"odds_ratio": float(odds), "p_value": float(p_f)},
                }
        return {
            "primary_result": primary,
            "assumption_checks": assumption_checks,
            "recommended_alternative": recommended_alternative,
            "diagnostic_plots": [],
            "learned_field_traits": [],
            "group_sizes": {g: int(np.sum(table[i])) for i, g in enumerate(g_levels)},
        }

    # Grouped numeric tests (2-group t/MWU or k-group ANOVA/Kruskal)
    grps = tbl.column(group_col).to_pylist()
    vals = tbl.column(value_col).to_pylist()
    grouped = {}
    for g, v in zip(grps, vals):
        if g is None or v is None:
            continue
        grouped.setdefault(str(g), []).append(float(v))

    g_names = sorted(grouped.keys())
    arrays = [np.asarray(grouped[g], dtype=np.float64) for g in g_names]
    group_sizes = {g: int(len(arr)) for g, arr in zip(g_names, arrays)}

    norm_failed = False
    for g, arr in zip(g_names, arrays):
        nc = _normality_check(arr, g)
        assumption_checks.append(nc)
        if nc["status"] == "failed":
            norm_failed = True

    if len(arrays) >= 2:
        lev_stat, lev_p = stats.levene(*arrays)
        lev_status = "passed" if float(lev_p) >= 0.05 else "failed"
        assumption_checks.append({
            "check": "equal_variance_levene",
            "status": lev_status,
            "statistic": round(float(lev_stat), 6),
            "p_value": round(float(lev_p), 6),
            "message": f"Levene test p={lev_p:.4g}"
        })

    if method in ("anova_oneway", "kruskal_wallis"):
        if method == "anova_oneway":
            f_stat, p_val = stats.f_oneway(*arrays)
            grand_mean = float(np.mean(np.concatenate(arrays)))
            ss_between = sum(len(a) * ((float(np.mean(a)) - grand_mean) ** 2) for a in arrays)
            ss_total = sum(float(np.sum((a - grand_mean) ** 2)) for a in arrays)
            eta2 = ss_between / ss_total if ss_total > 0.0 else 0.0
            primary = {
                "method": "anova_oneway",
                "test_statistic": float(f_stat),
                "p_value": float(p_val),
                "effect_size": {"metric": "eta_squared", "value": round(eta2, 6), "interpretation": _interpret_d(math.sqrt(max(0.0, eta2)) * 2.0)},
                "confidence_interval": None,
            }
            if norm_failed:
                h_stat, kw_p = stats.kruskal(*arrays)
                recommended_alternative = {
                    "method": "kruskal_wallis",
                    "reason": "One or more groups failed normality; Kruskal-Wallis rank test computed.",
                    "result": {"test_statistic": float(h_stat), "p_value": float(kw_p)},
                }
        else:
            h_stat, kw_p = stats.kruskal(*arrays)
            primary = {
                "method": "kruskal_wallis",
                "test_statistic": float(h_stat),
                "p_value": float(kw_p),
                "effect_size": {"metric": "kruskal_h", "value": float(h_stat), "interpretation": "non_parametric"},
                "confidence_interval": None,
            }
    else:
        # Two-group test: group 1 vs group 0 (e.g. treatment - control when sorted: control, treatment)
        # Use alphabetical second minus first so treatment - control is positive when treatment > control
        a_name, b_name = g_names[1], g_names[0]
        a, b = arrays[1], arrays[0]
        primary = _run_two_sample(method, a, b, alpha, alt)
        primary["groups_compared"] = {"group_a": a_name, "group_b": b_name}

        if method in ("welch_t", "student_t") and norm_failed:
            mwu = _run_two_sample("mann_whitney_u", a, b, alpha, alt)
            recommended_alternative = {
                "method": "mann_whitney_u",
                "reason": "Non-normal heavy-tailed distribution detected; Mann-Whitney U rank test computed alongside parametric test.",
                "result": mwu,
            }
            all_vals = np.concatenate([a, b])
            skew_all = float(stats.skew(all_vals, bias=False))
            kurt_all = float(stats.kurtosis(all_vals, fisher=True, bias=False))
            learned_field_traits.append({
                "field_canonical_id": f"{dataset_name}.{value_col}",
                "distribution_family": "log_normal_heavy_tailed" if skew_all > 1.5 else "non_normal",
                "is_heavy_tailed": bool(abs(skew_all) > 1.5 or kurt_all > 3.0),
                "skewness": round(skew_all, 4),
                "kurtosis": round(kurt_all, 4),
                "seasonality_period": None,
                "recommended_test_family": "non_parametric_or_log_transform",
                "known_confounders": [f"{dataset_name}.{stratify_by}"] if stratify_by else [],
                "epistemic_status": "empirically_tested",
                "confidence": 0.93,
            })

        # Stratified confounding / Simpson's paradox check
        if stratify_by:
            strata_vals = tbl.column(stratify_by).to_pylist()
            by_stratum = {}
            for st, g, v in zip(strata_vals, grps, vals):
                if st is None or g is None or v is None:
                    continue
                by_stratum.setdefault(str(st), {}).setdefault(str(g), []).append(float(v))

            agg_diff = float(np.mean(a) - np.mean(b))
            stratum_diffs = []
            for st_name in sorted(by_stratum.keys()):
                if a_name in by_stratum[st_name] and b_name in by_stratum[st_name]:
                    sa = np.asarray(by_stratum[st_name][a_name], dtype=np.float64)
                    sb = np.asarray(by_stratum[st_name][b_name], dtype=np.float64)
                    s_diff = float(np.mean(sa) - np.mean(sb))
                    s_res = _run_two_sample(method, sa, sb, alpha, alt)
                    stratum_diffs.append({
                        "stratum": st_name,
                        "mean_a": float(np.mean(sa)),
                        "mean_b": float(np.mean(sb)),
                        "diff": round(s_diff, 6),
                        "n_a": int(len(sa)),
                        "n_b": int(len(sb)),
                        "p_value": s_res["p_value"],
                    })

            simpson = False
            if len(stratum_diffs) >= 2 and agg_diff != 0.0:
                if agg_diff > 0.0 and all(sd["diff"] < 0.0 for sd in stratum_diffs):
                    simpson = True
                elif agg_diff < 0.0 and all(sd["diff"] > 0.0 for sd in stratum_diffs):
                    simpson = True

            primary["stratified_analysis"] = {
                "stratify_by": stratify_by,
                "aggregate_diff": round(agg_diff, 6),
                "strata": stratum_diffs,
                "simpsons_paradox_detected": simpson,
            }
            assumption_checks.append({
                "check": f"confounding_simpsons_paradox_{stratify_by}",
                "status": "failed" if simpson else "passed",
                "statistic": round(agg_diff, 6),
                "p_value": None,
                "message": (
                    f"Simpson's paradox detected across '{stratify_by}': aggregate diff ({agg_diff:+.4f}) reverses direction in every stratum!"
                    if simpson
                    else f"Stratified direction across '{stratify_by}' is consistent with aggregate effect."
                ),
            })
            if simpson:
                learned_field_traits.append({
                    "field_canonical_id": f"{dataset_name}.{value_col}",
                    "distribution_family": None,
                    "is_heavy_tailed": False,
                    "skewness": None,
                    "kurtosis": None,
                    "seasonality_period": None,
                    "recommended_test_family": "stratified_mantel_haenszel",
                    "known_confounders": [f"{dataset_name}.{stratify_by}"],
                    "epistemic_status": "empirically_tested",
                    "confidence": 0.96,
                })

        # Plot specs
        hist_series = []
        for g_lbl, arr in [(a_name, a), (b_name, b)]:
            cnts, edgs = np.histogram(arr, bins=20)
            hist_series.append({
                "name": g_lbl,
                "bins": [round(float(e), 6) for e in edgs[:max_pts]],
                "counts": [int(c) for c in cnts[:max_pts]],
            })
        diagnostic_plots.append({
            "plot_type": "distribution_histogram",
            "title": f"Group Distributions ({value_col} by {group_col})",
            "series": hist_series,
        })
        ci = primary.get("confidence_interval")
        if ci:
            diagnostic_plots.append({
                "plot_type": "ci_forest",
                "title": f"Difference & {int((1.0 - alpha) * 100)}% CI ({a_name} - {b_name})",
                "series": [{
                    "name": f"{a_name} - {b_name}",
                    "estimate": round(float(np.mean(a) - np.mean(b)), 6),
                    "lower": ci["lower"],
                    "upper": ci["upper"],
                }]
            })

    return {
        "primary_result": primary,
        "assumption_checks": assumption_checks,
        "recommended_alternative": recommended_alternative,
        "diagnostic_plots": diagnostic_plots,
        "learned_field_traits": learned_field_traits,
        "group_sizes": group_sizes,
    }
"#;

pub const BUILTIN_STAT_POWER_CODE: &str = r#"
import math
import numpy as np
from statsmodels.stats.power import TTestIndPower, TTestPower, NormalIndPower, FTestAnovaPower
from statsmodels.stats.proportion import proportion_effectsize

def run(inputs, params):
    test_type = params.get("test_type", "two_sample_mean")
    solve_for = params.get("solve_for", "sample_size")
    alpha = float(params.get("alpha", 0.05))
    target_power = params.get("power")
    effect_size = params.get("effect_size")
    p1 = params.get("p1")
    p2 = params.get("p2")
    n_per_group = params.get("n_per_group")
    ratio = float(params.get("ratio", 1.0))
    num_groups = int(params.get("num_groups", 2))
    alt = params.get("alternative", "two-sided")

    if test_type == "two_sample_proportion":
        if effect_size is None and p1 is not None and p2 is not None:
            effect_size = abs(float(proportion_effectsize(float(p1), float(p2))))
        solver = NormalIndPower()
    elif test_type == "paired_mean":
        solver = TTestPower()
    elif test_type == "oneway_anova":
        solver = FTestAnovaPower()
    else:
        solver = TTestIndPower()

    eff = float(effect_size) if effect_size is not None else None
    pwr = float(target_power) if target_power is not None else None
    nobs = float(n_per_group) if n_per_group is not None else None

    if solve_for == "sample_size":
        if test_type == "oneway_anova":
            solved_n = float(solver.solve_power(effect_size=eff, nobs=None, alpha=alpha, power=pwr, k_groups=num_groups))
        elif test_type == "paired_mean":
            solved_n = float(solver.solve_power(effect_size=eff, nobs=None, alpha=alpha, power=pwr, alternative=alt))
        else:
            solved_n = float(solver.solve_power(effect_size=eff, nobs1=None, alpha=alpha, power=pwr, ratio=ratio, alternative=alt))
        result = {"required_n_per_group": int(math.ceil(solved_n)), "exact_n": round(solved_n, 4), "power": pwr, "effect_size": eff}
        curve_eff = eff
    elif solve_for == "mde":
        if test_type == "oneway_anova":
            solved_eff = float(solver.solve_power(effect_size=None, nobs=nobs, alpha=alpha, power=pwr, k_groups=num_groups))
        elif test_type == "paired_mean":
            solved_eff = float(solver.solve_power(effect_size=None, nobs=nobs, alpha=alpha, power=pwr, alternative=alt))
        else:
            solved_eff = float(solver.solve_power(effect_size=None, nobs1=nobs, alpha=alpha, power=pwr, ratio=ratio, alternative=alt))
        result = {"mde_effect_size": round(solved_eff, 6), "n_per_group": int(nobs), "power": pwr}
        curve_eff = solved_eff
    else:
        if test_type == "oneway_anova":
            solved_pwr = float(solver.solve_power(effect_size=eff, nobs=nobs, alpha=alpha, power=None, k_groups=num_groups))
        elif test_type == "paired_mean":
            solved_pwr = float(solver.solve_power(effect_size=eff, nobs=nobs, alpha=alpha, power=None, alternative=alt))
        else:
            solved_pwr = float(solver.solve_power(effect_size=eff, nobs1=nobs, alpha=alpha, power=None, ratio=ratio, alternative=alt))
        result = {"achieved_power": round(solved_pwr, 6), "n_per_group": int(nobs), "effect_size": eff}
        curve_eff = eff

    assumption_checks = []
    check_pwr = result.get("achieved_power", pwr if pwr is not None else 0.8)
    assumption_checks.append({
        "check": "statistical_power_adequacy",
        "status": "passed" if check_pwr >= 0.80 else "warning",
        "statistic": round(float(check_pwr), 4),
        "p_value": None,
        "message": f"Power = {check_pwr:.3f} at alpha={alpha} ({'adequate >= 0.80' if check_pwr >= 0.80 else 'underpowered < 0.80'})."
    })

    # Power curve points
    n_grid = [10, 25, 50, 100, 250, 500, 1000]
    curve_pts = []
    for ng in n_grid:
        if test_type == "oneway_anova":
            pv = float(solver.solve_power(effect_size=curve_eff, nobs=float(ng), alpha=alpha, power=None, k_groups=num_groups))
        elif test_type == "paired_mean":
            pv = float(solver.solve_power(effect_size=curve_eff, nobs=float(ng), alpha=alpha, power=None, alternative=alt))
        else:
            pv = float(solver.solve_power(effect_size=curve_eff, nobs1=float(ng), alpha=alpha, power=None, ratio=ratio, alternative=alt))
        curve_pts.append([ng, round(pv, 4)])

    result["test_type"] = test_type
    result["solve_for"] = solve_for
    result["alpha"] = alpha

    return {
        "primary_result": result,
        "assumption_checks": assumption_checks,
        "recommended_alternative": None,
        "diagnostic_plots": [{
            "plot_type": "power_curve",
            "title": f"Power Curve ({test_type}, effect_size={round(curve_eff, 4)})",
            "series": [{"name": "power_vs_n", "points": curve_pts}],
        }],
        "learned_field_traits": [],
        "group_sizes": {},
    }
"#;

pub const BUILTIN_STAT_CORRELATION_VIF_CODE: &str = r#"
import math
import numpy as np
from scipy import stats
import statsmodels.api as sm
from statsmodels.stats.outliers_influence import variance_inflation_factor
from statsmodels.stats.multitest import multipletests

def run(inputs, params):
    tbl = inputs["data"]
    columns = params.get("columns", [])
    methods = params.get("methods", ["pearson", "spearman"])
    control_cols = params.get("control_cols", [])
    correction = params.get("correction", "fdr_bh")

    all_cols = list(dict.fromkeys(list(columns) + list(control_cols)))
    raw_cols = {c: tbl.column(c).to_pylist() for c in all_cols}
    n_total = len(raw_cols[all_cols[0]]) if all_cols else 0

    valid_idx = [
        i for i in range(n_total)
        if all(raw_cols[c][i] is not None and not (isinstance(raw_cols[c][i], float) and math.isnan(raw_cols[c][i])) for c in all_cols)
    ]
    mat = {c: np.asarray([float(raw_cols[c][i]) for i in valid_idx], dtype=np.float64) for c in all_cols}
    n = len(valid_idx)

    pairs = []
    raw_pvals = []
    assumption_checks = []

    for i in range(len(columns)):
        for j in range(i + 1, len(columns)):
            c1, c2 = columns[i], columns[j]
            x, y = mat[c1], mat[c2]
            entry = {"col_a": c1, "col_b": c2, "n": n}
            if "pearson" in methods:
                r, p = stats.pearsonr(x, y)
                entry["pearson_r"] = float(r)
                entry["pearson_p"] = float(p)
                raw_pvals.append(float(p))
            if "spearman" in methods:
                rho, p_s = stats.spearmanr(x, y)
                entry["spearman_rho"] = float(rho)
                entry["spearman_p"] = float(p_s)
            if "kendall" in methods:
                tau, p_k = stats.kendalltau(x, y)
                entry["kendall_tau"] = float(tau)
                entry["kendall_p"] = float(p_k)

            if "pearson_r" in entry and "spearman_rho" in entry:
                div = abs(entry["pearson_r"] - entry["spearman_rho"])
                if div > 0.15:
                    assumption_checks.append({
                        "check": f"pearson_spearman_divergence_{c1}_{c2}",
                        "status": "warning",
                        "statistic": round(div, 4),
                        "p_value": None,
                        "message": f"|r - rho| = {div:.3f} > 0.15 between '{c1}' and '{c2}' (possible outliers or non-linearity)."
                    })

            if control_cols:
                ctrl_mat = sm.add_constant(np.column_stack([mat[cc] for cc in control_cols]))
                res_x = sm.OLS(x, ctrl_mat).fit().resid
                res_y = sm.OLS(y, ctrl_mat).fit().resid
                pr, pp = stats.pearsonr(res_x, res_y)
                entry["partial_pearson_r"] = float(pr)
                entry["partial_pearson_p"] = float(pp)

            pairs.append(entry)

    if raw_pvals and correction and correction != "none":
        _, p_adj, _, _ = multipletests(raw_pvals, method=correction)
        for entry, pa in zip(pairs, p_adj):
            entry["pearson_p_adjusted"] = float(pa)

    vif_map = {}
    cond_num = None
    if len(columns) >= 2 and n > len(columns) + 1:
        X = np.column_stack([mat[c] for c in columns])
        X_const = sm.add_constant(X)
        cond_num = float(np.linalg.cond(X_const))
        for idx, c in enumerate(columns):
            vif_val = float(variance_inflation_factor(X_const, idx + 1))
            vif_map[c] = vif_val
            if vif_val > 10.0:
                assumption_checks.append({
                    "check": f"vif_multicollinearity_{c}",
                    "status": "failed",
                    "statistic": round(vif_val, 4),
                    "p_value": None,
                    "message": f"Severe multicollinearity: VIF({c}) = {vif_val:.2f} > 10."
                })
            elif vif_val > 5.0:
                assumption_checks.append({
                    "check": f"vif_multicollinearity_{c}",
                    "status": "warning",
                    "statistic": round(vif_val, 4),
                    "p_value": None,
                    "message": f"Moderate multicollinearity: VIF({c}) = {vif_val:.2f} > 5."
                })

    if not assumption_checks:
        assumption_checks.append({
            "check": "correlation_and_vif_diagnostics",
            "status": "passed",
            "statistic": round(max(vif_map.values()) if vif_map else 1.0, 4),
            "p_value": None,
            "message": "No severe multicollinearity (VIF <= 5) or Pearson/Spearman divergence detected."
        })

    return {
        "primary_result": {
            "n": n,
            "adjustment_method": correction if correction != "none" else None,
            "pairs": pairs,
            "vif": vif_map,
            "condition_number": cond_num,
        },
        "assumption_checks": assumption_checks,
        "recommended_alternative": None,
        "diagnostic_plots": [{
            "plot_type": "correlation_heatmap",
            "title": "Pairwise Correlation Matrix",
            "series": pairs,
        }],
        "learned_field_traits": [],
        "group_sizes": {},
    }
"#;

pub const BUILTIN_STAT_REGRESSION_CODE: &str = r#"
import math
import numpy as np
from scipy import stats
import statsmodels.api as sm
from statsmodels.stats.diagnostic import het_breuschpagan
from statsmodels.stats.stattools import durbin_watson
from statsmodels.stats.outliers_influence import variance_inflation_factor

def _downsample_pairs(xs, ys, max_pts):
    n = len(xs)
    if n <= max_pts:
        return [[round(float(a), 6), round(float(b), 6)] for a, b in zip(xs, ys)]
    idx = np.linspace(0, n - 1, max_pts, dtype=int)
    return [[round(float(xs[i]), 6), round(float(ys[i]), 6)] for i in idx]

def run(inputs, params):
    tbl = inputs["data"]
    model_type = params.get("model_type", "ols")
    target_col = params["target_col"]
    feature_cols = params["feature_cols"]
    add_intercept = bool(params.get("add_intercept", True))
    max_pts = int(params.get("max_plot_points", 1000))

    cols = [target_col] + list(feature_cols)
    raw = {c: tbl.column(c).to_pylist() for c in cols}
    n_total = len(raw[target_col])
    valid_idx = [
        i for i in range(n_total)
        if all(raw[c][i] is not None and not (isinstance(raw[c][i], float) and math.isnan(raw[c][i])) for c in cols)
    ]
    y = np.asarray([float(raw[target_col][i]) for i in valid_idx], dtype=np.float64)
    X_raw = np.column_stack([np.asarray([float(raw[c][i]) for i in valid_idx], dtype=np.float64) for c in feature_cols])
    n = len(y)

    if add_intercept:
        X = sm.add_constant(X_raw, has_constant="add")
        param_names = ["const"] + list(feature_cols)
    else:
        X = X_raw
        param_names = list(feature_cols)

    assumption_checks = []
    recommended_alternative = None

    if model_type == "ols":
        fit = sm.OLS(y, X).fit()
        hc3_fit = fit.get_robustcov_results(cov_type="HC3")
        fitted = np.asarray(fit.fittedvalues, dtype=np.float64)
        resid = np.asarray(fit.resid, dtype=np.float64)
        ci_arr = fit.conf_int()

        coefs = []
        for idx, pname in enumerate(param_names):
            coefs.append({
                "name": pname,
                "estimate": float(fit.params[idx]),
                "std_err": float(fit.bse[idx]),
                "hc3_std_err": float(hc3_fit.bse[idx]),
                "t_stat": float(fit.tvalues[idx]),
                "p_value": float(fit.pvalues[idx]),
                "hc3_p_value": float(hc3_fit.pvalues[idx]),
                "ci_lower": float(ci_arr[idx, 0]),
                "ci_upper": float(ci_arr[idx, 1]),
            })

        # Breusch-Pagan heteroscedasticity
        bp_stat, bp_p, _, _ = het_breuschpagan(resid, X)
        bp_status = "passed" if float(bp_p) >= 0.05 else "failed"
        assumption_checks.append({
            "check": "homoscedasticity_breusch_pagan",
            "status": bp_status,
            "statistic": round(float(bp_stat), 6),
            "p_value": round(float(bp_p), 6),
            "message": f"Breusch-Pagan LM={bp_stat:.4f}, p={bp_p:.4g}"
        })

        # Durbin-Watson autocorrelation
        dw = float(durbin_watson(resid))
        dw_status = "passed" if 1.5 <= dw <= 2.5 else "warning"
        assumption_checks.append({
            "check": "residual_autocorrelation_durbin_watson",
            "status": dw_status,
            "statistic": round(dw, 4),
            "p_value": None,
            "message": f"Durbin-Watson = {dw:.3f} (ideal ~2.0)"
        })

        # Residual normality
        if 3 <= n <= 5000:
            rn = stats.shapiro(resid)
            rn_stat, rn_p = float(rn.statistic), float(rn.pvalue)
        else:
            rn = stats.normaltest(resid)
            rn_stat, rn_p = float(rn.statistic), float(rn.pvalue)
        assumption_checks.append({
            "check": "residual_normality",
            "status": "passed" if rn_p >= 0.05 else "warning",
            "statistic": round(rn_stat, 6),
            "p_value": round(rn_p, 6),
            "message": f"Residual normality p={rn_p:.4g}"
        })

        # Cook's distance influential points
        infl = fit.get_influence()
        cooks_d, _ = infl.cooks_distance
        cooks_thresh = 4.0 / float(n)
        influential_count = int(np.sum(cooks_d > cooks_thresh))
        assumption_checks.append({
            "check": "influential_outliers_cooks_d",
            "status": "passed" if influential_count == 0 else "warning",
            "statistic": float(influential_count),
            "p_value": None,
            "message": f"{influential_count} observations exceed Cook's D threshold ({cooks_thresh:.4f})."
        })

        # VIF
        vif_map = {}
        if len(feature_cols) >= 2:
            X_vif = sm.add_constant(X_raw, has_constant="add")
            for idx, fname in enumerate(feature_cols):
                vv = float(variance_inflation_factor(X_vif, idx + 1))
                vif_map[fname] = vv
                if vv > 10.0:
                    assumption_checks.append({
                        "check": f"vif_{fname}",
                        "status": "failed",
                        "statistic": round(vv, 4),
                        "p_value": None,
                        "message": f"Severe multicollinearity on '{fname}': VIF={vv:.2f}"
                    })

        if bp_status == "failed":
            hc3_ci = hc3_fit.conf_int()
            recommended_alternative = {
                "method": "ols_hc3_robust_se",
                "reason": "Breusch-Pagan test detected heteroscedasticity (p < 0.05); HC3 heteroscedasticity-robust standard errors computed.",
                "result": {
                    "coefficients": [
                        {
                            "name": pname,
                            "estimate": float(hc3_fit.params[idx]),
                            "hc3_std_err": float(hc3_fit.bse[idx]),
                            "hc3_p_value": float(hc3_fit.pvalues[idx]),
                            "ci_lower": float(hc3_ci[idx, 0]),
                            "ci_upper": float(hc3_ci[idx, 1]),
                        }
                        for idx, pname in enumerate(param_names)
                    ]
                }
            }

        primary = {
            "model_type": "ols",
            "n": n,
            "r_squared": float(fit.rsquared),
            "adj_r_squared": float(fit.rsquared_adj),
            "aic": float(fit.aic),
            "bic": float(fit.bic),
            "f_statistic": float(fit.fvalue) if fit.fvalue is not None else None,
            "f_pvalue": float(fit.f_pvalue) if fit.f_pvalue is not None else None,
            "residual_std_err": float(math.sqrt(fit.mse_resid)),
            "coefficients": coefs,
            "vif": vif_map,
            "influential_count": influential_count,
        }
    elif model_type == "huber_robust":
        fit = sm.RLM(y, X, M=sm.robust.norms.HuberT()).fit()
        fitted = np.asarray(fit.fittedvalues, dtype=np.float64)
        resid = np.asarray(fit.resid, dtype=np.float64)
        ci_arr = fit.conf_int()
        coefs = [
            {
                "name": pname,
                "estimate": float(fit.params[idx]),
                "std_err": float(fit.bse[idx]),
                "p_value": float(fit.pvalues[idx]),
                "ci_lower": float(ci_arr[idx, 0]),
                "ci_upper": float(ci_arr[idx, 1]),
            }
            for idx, pname in enumerate(param_names)
        ]
        primary = {"model_type": "huber_robust", "n": n, "scale": float(fit.scale), "coefficients": coefs}
    else:
        fam_map = {
            "glm_gaussian": sm.families.Gaussian(),
            "glm_binomial": sm.families.Binomial(),
            "glm_poisson": sm.families.Poisson(),
            "glm_gamma": sm.families.Gamma(),
        }
        fam = fam_map.get(model_type, sm.families.Gaussian())
        fit = sm.GLM(y, X, family=fam).fit()
        fitted = np.asarray(fit.fittedvalues, dtype=np.float64)
        resid = np.asarray(fit.resid_deviance, dtype=np.float64)
        ci_arr = fit.conf_int()
        coefs = [
            {
                "name": pname,
                "estimate": float(fit.params[idx]),
                "std_err": float(fit.bse[idx]),
                "p_value": float(fit.pvalues[idx]),
                "ci_lower": float(ci_arr[idx, 0]),
                "ci_upper": float(ci_arr[idx, 1]),
            }
            for idx, pname in enumerate(param_names)
        ]
        primary = {
            "model_type": model_type,
            "n": n,
            "deviance": float(fit.deviance),
            "aic": float(fit.aic),
            "coefficients": coefs,
        }

    sorted_r = np.sort(resid)
    probs = (np.arange(1, len(sorted_r) + 1) - 0.5) / float(len(sorted_r))
    theo_q = stats.norm.ppf(probs)

    diagnostic_plots = [
        {
            "plot_type": "residual_vs_fitted",
            "title": f"Residuals vs Fitted ({target_col})",
            "series": [{"name": "residuals", "points": _downsample_pairs(fitted, resid, max_pts)}],
        },
        {
            "plot_type": "qq_plot",
            "title": "Residual Normal Q-Q Plot",
            "series": [{"name": "residuals", "points": _downsample_pairs(theo_q, sorted_r, max_pts)}],
        },
        {
            "plot_type": "ci_forest",
            "title": "Model Coefficients & 95% CI",
            "series": [
                {"name": c["name"], "estimate": c["estimate"], "lower": c["ci_lower"], "upper": c["ci_upper"]}
                for c in primary["coefficients"]
            ],
        },
    ]

    return {
        "primary_result": primary,
        "assumption_checks": assumption_checks,
        "recommended_alternative": recommended_alternative,
        "diagnostic_plots": diagnostic_plots,
        "learned_field_traits": [],
        "group_sizes": {},
    }
"#;

pub const BUILTIN_STAT_TIMESERIES_CODE: &str = r#"
import math
import numpy as np
from scipy import stats
from statsmodels.tsa.seasonal import STL
from statsmodels.tsa.stattools import adfuller, kpss, acf, pacf
from statsmodels.stats.diagnostic import acorr_ljungbox
from statsmodels.tsa.holtwinters import ExponentialSmoothing
from statsmodels.tsa.arima.model import ARIMA

def _downsample_1d(vals, max_pts):
    n = len(vals)
    if n <= max_pts:
        return [round(float(v), 6) for v in vals]
    idx = np.linspace(0, n - 1, max_pts, dtype=int)
    return [round(float(vals[i]), 6) for i in idx]

def _time_sort_key(t):
    if isinstance(t, (int, float)):
        return (0, float(t), "")
    return (1, 0.0, str(t))

def run(inputs, params):
    tbl = inputs["data"]
    time_col = params["time_col"]
    value_col = params["value_col"]
    compare_col = params.get("compare_col")
    period = params.get("period")
    forecast_horizon = params.get("forecast_horizon")
    model_name = params.get("model", "ets")
    anomaly_z = float(params.get("anomaly_z_threshold", 3.0))
    dataset_name = params.get("dataset_name", "dataset")
    max_pts = int(params.get("max_plot_points", 1000))

    times = tbl.column(time_col).to_pylist()
    raw_vals = tbl.column(value_col).to_pylist()
    pairs = [(t, float(v)) for t, v in zip(times, raw_vals) if t is not None and v is not None]
    pairs.sort(key=lambda kv: _time_sort_key(kv[0]))
    y = np.asarray([kv[1] for kv in pairs], dtype=np.float64)
    n = len(y)

    assumption_checks = []
    recommended_alternative = None
    diagnostic_plots = []
    learned_field_traits = []

    # ADF & KPSS stationarity tests
    adf_res = adfuller(y, autolag="AIC")
    adf_stat, adf_p = float(adf_res[0]), float(adf_res[1])
    kpss_stat, kpss_p, _, _ = kpss(y, regression="c", nlags="auto")
    kpss_stat, kpss_p = float(kpss_stat), float(kpss_p)

    is_stationary = bool(adf_p < 0.05 and kpss_p >= 0.05)
    assumption_checks.append({
        "check": "stationarity_adf_kpss",
        "status": "passed" if is_stationary else "failed",
        "statistic": round(adf_stat, 6),
        "p_value": round(adf_p, 6),
        "message": f"ADF p={adf_p:.4g}, KPSS p={kpss_p:.4g} ({'stationary' if is_stationary else 'non-stationary: difference before correlating'})."
    })

    # STL Decomposition
    stl_out = None
    anomalies = []
    if period is not None and int(period) >= 2 and n >= int(period) * 2:
        p_int = int(period)
        stl_res = STL(y, period=p_int, robust=True).fit()
        trend = np.asarray(stl_res.trend, dtype=np.float64)
        seasonal = np.asarray(stl_res.seasonal, dtype=np.float64)
        resid = np.asarray(stl_res.resid, dtype=np.float64)

        var_resid = float(np.var(resid))
        var_sr = float(np.var(seasonal + resid))
        seasonal_strength = max(0.0, 1.0 - var_resid / var_sr) if var_sr > 0.0 else 0.0

        med_r = float(np.median(resid))
        mad_r = float(np.median(np.abs(resid - med_r)))
        if mad_r > 0.0:
            mz = 0.6745 * (resid - med_r) / mad_r
            for i, z_val in enumerate(mz):
                if abs(float(z_val)) > anomaly_z:
                    anomalies.append({"index": i, "time": str(pairs[i][0]), "value": float(y[i]), "z_score": round(float(z_val), 3)})

        stl_out = {
            "period": p_int,
            "seasonal_strength": round(seasonal_strength, 6),
            "anomaly_count": len(anomalies),
            "anomalies": anomalies[:50],
        }
        diagnostic_plots.append({
            "plot_type": "stl_decomposition",
            "title": f"STL Decomposition ({value_col}, period={p_int})",
            "series": [
                {"name": "observed", "values": _downsample_1d(y, max_pts)},
                {"name": "trend", "values": _downsample_1d(trend, max_pts)},
                {"name": "seasonal", "values": _downsample_1d(seasonal, max_pts)},
                {"name": "remainder", "values": _downsample_1d(resid, max_pts)},
            ]
        })

    # ACF & PACF
    nlags = min(20, max(2, n // 4))
    acf_vals = [round(float(v), 6) for v in acf(y, nlags=nlags, fft=True)]
    pacf_vals = [round(float(v), 6) for v in pacf(y, nlags=min(nlags, max(2, n // 2 - 2)))]
    bartlett_band = round(1.96 / math.sqrt(float(n)), 6) if n > 0 else 0.0
    diagnostic_plots.append({
        "plot_type": "acf_pacf",
        "title": f"ACF & PACF ({value_col})",
        "series": [
            {"name": "acf", "values": acf_vals},
            {"name": "pacf", "values": pacf_vals},
            {"name": "confidence_band", "value": bartlett_band},
        ]
    })

    # Ljung-Box on first differences
    dy = np.diff(y)
    lb_df = acorr_ljungbox(dy, lags=[min(10, max(1, len(dy) // 5))], return_df=True)
    lb_p = float(lb_df["lb_pvalue"].iloc[-1])

    # Cross-series spurious correlation check
    cross_corr = None
    if compare_col:
        raw_c = tbl.column(compare_col).to_pylist()
        c_pairs = [(t, float(v1), float(v2)) for t, v1, v2 in zip(times, raw_vals, raw_c) if t is not None and v1 is not None and v2 is not None]
        c_pairs.sort(key=lambda kv: _time_sort_key(kv[0]))
        s1 = np.asarray([kv[1] for kv in c_pairs], dtype=np.float64)
        s2 = np.asarray([kv[2] for kv in c_pairs], dtype=np.float64)
        level_r, level_p = stats.pearsonr(s1, s2)
        diff_r, diff_p = stats.pearsonr(np.diff(s1), np.diff(s2))
        adf2_p = float(adfuller(s2, autolag="AIC")[1])
        spurious = bool((adf_p >= 0.05 or adf2_p >= 0.05) and abs(float(level_r)) > 0.5 and abs(float(diff_r)) < 0.25)
        cross_corr = {
            "compare_col": compare_col,
            "level_pearson_r": round(float(level_r), 6),
            "level_p_value": float(level_p),
            "first_diff_pearson_r": round(float(diff_r), 6),
            "first_diff_p_value": float(diff_p),
            "compare_adf_p_value": round(adf2_p, 6),
            "spurious_nonstationary_correlation": spurious,
        }
        if spurious:
            assumption_checks.append({
                "check": "spurious_nonstationary_correlation",
                "status": "failed",
                "statistic": round(float(level_r), 6),
                "p_value": float(diff_p),
                "message": f"Spurious level correlation detected (level r={level_r:.3f}, first-differenced r={diff_r:.3f}, p={diff_p:.4g})."
            })
            recommended_alternative = {
                "method": "first_differenced_correlation",
                "reason": "Non-stationary series detected by ADF/KPSS; use first-differenced correlation instead of level correlation.",
                "result": {
                    "first_diff_pearson_r": round(float(diff_r), 6),
                    "first_diff_p_value": float(diff_p),
                }
            }

    # Optional ETS / ARIMA forecast
    forecast_out = None
    if forecast_horizon is not None and int(forecast_horizon) > 0:
        h = int(forecast_horizon)
        if model_name == "arima":
            fit = ARIMA(y, order=(1, 1 if not is_stationary else 0, 1)).fit()
            fc = fit.get_forecast(steps=h)
            mean_fc = np.asarray(fc.predicted_mean, dtype=np.float64)
            ci95 = np.asarray(fc.conf_int(alpha=0.05), dtype=np.float64)
            ci80 = np.asarray(fc.conf_int(alpha=0.20), dtype=np.float64)
            forecast_out = {
                "model": "arima",
                "horizon": h,
                "mean": [round(float(v), 6) for v in mean_fc],
                "lower_80": [round(float(v), 6) for v in ci80[:, 0]],
                "upper_80": [round(float(v), 6) for v in ci80[:, 1]],
                "lower_95": [round(float(v), 6) for v in ci95[:, 0]],
                "upper_95": [round(float(v), 6) for v in ci95[:, 1]],
            }
        else:
            seasonal_arg = "add" if (period is not None and int(period) >= 2 and n >= int(period) * 2) else None
            fit = ExponentialSmoothing(
                y,
                trend="add",
                seasonal=seasonal_arg,
                seasonal_periods=int(period) if seasonal_arg else None,
            ).fit()
            mean_fc = np.asarray(fit.forecast(h), dtype=np.float64)
            resid_sd = float(np.std(fit.resid, ddof=1)) if len(fit.resid) > 1 else 0.0
            steps = np.arange(1, h + 1, dtype=np.float64)
            se_h = resid_sd * np.sqrt(steps)
            forecast_out = {
                "model": "ets",
                "horizon": h,
                "mean": [round(float(v), 6) for v in mean_fc],
                "lower_80": [round(float(m - 1.2816 * s), 6) for m, s in zip(mean_fc, se_h)],
                "upper_80": [round(float(m + 1.2816 * s), 6) for m, s in zip(mean_fc, se_h)],
                "lower_95": [round(float(m - 1.96 * s), 6) for m, s in zip(mean_fc, se_h)],
                "upper_95": [round(float(m + 1.96 * s), 6) for m, s in zip(mean_fc, se_h)],
            }
        diagnostic_plots.append({
            "plot_type": "forecast_interval",
            "title": f"{model_name.upper()} Forecast ({value_col}, h={h})",
            "series": [forecast_out],
        })

    learned_field_traits.append({
        "field_canonical_id": f"{dataset_name}.{value_col}",
        "distribution_family": "stationary_series" if is_stationary else "non_stationary_series",
        "is_heavy_tailed": False,
        "skewness": None,
        "kurtosis": None,
        "seasonality_period": str(period) if (stl_out and stl_out["seasonal_strength"] > 0.3) else None,
        "recommended_test_family": "first_differenced_ts" if not is_stationary else "stationary_ts",
        "known_confounders": [],
        "epistemic_status": "empirically_tested",
        "confidence": 0.90,
    })

    return {
        "primary_result": {
            "n": n,
            "stationarity": {
                "adf_statistic": round(adf_stat, 6),
                "adf_p_value": round(adf_p, 6),
                "kpss_statistic": round(kpss_stat, 6),
                "kpss_p_value": round(kpss_p, 6),
                "is_stationary": is_stationary,
            },
            "ljung_box_diff_p_value": round(lb_p, 6),
            "stl": stl_out,
            "acf": acf_vals,
            "pacf": pacf_vals,
            "cross_series_correlation": cross_corr,
            "forecast": forecast_out,
        },
        "assumption_checks": assumption_checks,
        "recommended_alternative": recommended_alternative,
        "diagnostic_plots": diagnostic_plots,
        "learned_field_traits": learned_field_traits,
        "group_sizes": {},
    }
"#;
