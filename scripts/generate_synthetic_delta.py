#!/usr/bin/env python3
"""
Generate synthetic Delta Lake (Parquet) data for M4a Analysis Service PGO profiling.

Produces parquet files matching the schemas expected by delta_reader.rs:
  - metric_summaries/    — 5 experiments × 3 metrics × 2 variants, 10K aggregates
  - interleaving_scores/ — 2 experiments, 1K scored pairs
  - daily_treatment_effects/ — 2 experiments × 14 days
  - content_consumption/ — 2 experiments × 500 titles

Usage:
    python3 scripts/generate_synthetic_delta.py --output /tmp/pgo-delta
"""

import argparse
import os
import random
from datetime import date, timedelta

import pyarrow as pa
import pyarrow.parquet as pq


def generate_metric_summaries(output_dir: str) -> None:
    """5 experiments × 3 metrics × 2 variants with 10K user-level aggregates."""
    path = os.path.join(output_dir, "metric_summaries")
    os.makedirs(path, exist_ok=True)

    experiments = [f"pgo-exp-{i}" for i in range(1, 6)]
    metrics = ["watch_time_hours", "sessions_per_week", "retention_d7"]
    variants = ["control", "treatment"]
    base_date = date(2025, 3, 1)

    rows_per_combo = 333  # ~10K total rows across 5×3×2=30 combos
    records = {
        "experiment_id": [],
        "user_id": [],
        "variant_id": [],
        "metric_id": [],
        "lifecycle_segment": [],
        "metric_value": [],
        "cuped_covariate": [],
        "session_count": [],
        "computation_date": [],
    }

    segments = ["TRIAL", "NEW", "ESTABLISHED", "MATURE", "AT_RISK", None]

    for exp in experiments:
        for metric in metrics:
            for variant in variants:
                effect = 0.0 if variant == "control" else random.uniform(0.01, 0.15)
                for j in range(rows_per_combo):
                    records["experiment_id"].append(exp)
                    records["user_id"].append(f"user-{j}")
                    records["variant_id"].append(variant)
                    records["metric_id"].append(metric)
                    records["lifecycle_segment"].append(random.choice(segments))
                    base_val = random.gauss(10.0, 2.0)
                    records["metric_value"].append(base_val * (1.0 + effect))
                    records["cuped_covariate"].append(
                        base_val * random.uniform(0.8, 1.2) if random.random() > 0.3 else None
                    )
                    records["session_count"].append(random.randint(1, 30) if random.random() > 0.5 else None)
                    records["computation_date"].append(base_date)

    table = pa.table(
        {
            "experiment_id": pa.array(records["experiment_id"], type=pa.string()),
            "user_id": pa.array(records["user_id"], type=pa.string()),
            "variant_id": pa.array(records["variant_id"], type=pa.string()),
            "metric_id": pa.array(records["metric_id"], type=pa.string()),
            "lifecycle_segment": pa.array(records["lifecycle_segment"], type=pa.string()),
            "metric_value": pa.array(records["metric_value"], type=pa.float64()),
            "cuped_covariate": pa.array(records["cuped_covariate"], type=pa.float64()),
            "session_count": pa.array(records["session_count"], type=pa.int32()),
            "computation_date": pa.array(records["computation_date"], type=pa.date32()),
        }
    )
    pq.write_table(table, os.path.join(path, "data.parquet"))
    print(f"  metric_summaries: {len(records['experiment_id'])} rows")


def generate_interleaving_scores(output_dir: str) -> None:
    """2 experiments, 1K scored user pairs."""
    path = os.path.join(output_dir, "interleaving_scores")
    os.makedirs(path, exist_ok=True)

    experiments = ["pgo-interleave-1", "pgo-interleave-2"]
    algorithms = ["algo_a", "algo_b"]
    base_date = date(2025, 3, 1)

    # PyArrow MapArray requires keys and values arrays
    experiment_ids = []
    user_ids = []
    algo_keys_offsets = [0]
    algo_keys_values = []
    algo_score_values = []
    winning_algos = []
    total_engagements = []
    computation_dates = []

    for exp in experiments:
        for j in range(500):
            experiment_ids.append(exp)
            user_ids.append(f"user-{j}")
            scores = {alg: random.uniform(0, 1) for alg in algorithms}
            for alg in algorithms:
                algo_keys_values.append(alg)
                algo_score_values.append(scores[alg])
            algo_keys_offsets.append(algo_keys_offsets[-1] + len(algorithms))
            winner = max(scores, key=scores.get)
            winning_algos.append(winner)
            total_engagements.append(random.randint(1, 50))
            computation_dates.append(base_date)

    keys_array = pa.array(algo_keys_values, type=pa.string())
    items_array = pa.array(algo_score_values, type=pa.float64())
    offsets_array = pa.array(algo_keys_offsets, type=pa.int32())
    map_array = pa.MapArray.from_arrays(offsets_array, keys_array, items_array)

    table = pa.table(
        {
            "experiment_id": pa.array(experiment_ids, type=pa.string()),
            "user_id": pa.array(user_ids, type=pa.string()),
            "algorithm_scores": map_array,
            "winning_algorithm_id": pa.array(winning_algos, type=pa.string()),
            "total_engagements": pa.array(total_engagements, type=pa.int32()),
            "computation_date": pa.array(computation_dates, type=pa.date32()),
        }
    )
    pq.write_table(table, os.path.join(path, "data.parquet"))
    print(f"  interleaving_scores: {len(experiment_ids)} rows")


def generate_daily_treatment_effects(output_dir: str) -> None:
    """2 experiments × 14 days."""
    path = os.path.join(output_dir, "daily_treatment_effects")
    os.makedirs(path, exist_ok=True)

    experiments = ["pgo-exp-1", "pgo-exp-2"]
    metrics = ["watch_time_hours", "sessions_per_week"]
    base_date = date(2025, 2, 15)

    records = {
        "experiment_id": [],
        "metric_id": [],
        "effect_date": [],
        "treatment_mean": [],
        "control_mean": [],
        "absolute_effect": [],
        "sample_size": [],
    }

    for exp in experiments:
        for metric in metrics:
            true_effect = random.uniform(0.05, 0.20)
            for day_offset in range(14):
                d = base_date + timedelta(days=day_offset)
                ctrl = random.gauss(10.0, 0.5)
                treat = ctrl * (1.0 + true_effect) + random.gauss(0, 0.1)
                records["experiment_id"].append(exp)
                records["metric_id"].append(metric)
                records["effect_date"].append(d)
                records["treatment_mean"].append(treat)
                records["control_mean"].append(ctrl)
                records["absolute_effect"].append(treat - ctrl)
                records["sample_size"].append(random.randint(500, 5000))

    table = pa.table(
        {
            "experiment_id": pa.array(records["experiment_id"], type=pa.string()),
            "metric_id": pa.array(records["metric_id"], type=pa.string()),
            "effect_date": pa.array(records["effect_date"], type=pa.date32()),
            "treatment_mean": pa.array(records["treatment_mean"], type=pa.float64()),
            "control_mean": pa.array(records["control_mean"], type=pa.float64()),
            "absolute_effect": pa.array(records["absolute_effect"], type=pa.float64()),
            "sample_size": pa.array(records["sample_size"], type=pa.int64()),
        }
    )
    pq.write_table(table, os.path.join(path, "data.parquet"))
    print(f"  daily_treatment_effects: {len(records['experiment_id'])} rows")


def generate_content_consumption(output_dir: str) -> None:
    """2 experiments × 500 titles."""
    path = os.path.join(output_dir, "content_consumption")
    os.makedirs(path, exist_ok=True)

    experiments = ["pgo-interference-1", "pgo-interference-2"]
    variants = ["control", "treatment"]
    base_date = date(2025, 3, 1)

    records = {
        "experiment_id": [],
        "variant_id": [],
        "content_id": [],
        "watch_time_seconds": [],
        "view_count": [],
        "unique_viewers": [],
        "computation_date": [],
    }

    for exp in experiments:
        for variant in variants:
            for title_idx in range(250):
                records["experiment_id"].append(exp)
                records["variant_id"].append(variant)
                records["content_id"].append(f"title-{title_idx:04d}")
                records["watch_time_seconds"].append(random.uniform(100, 50000))
                views = random.randint(10, 10000)
                records["view_count"].append(views)
                records["unique_viewers"].append(random.randint(5, views))
                records["computation_date"].append(base_date)

    table = pa.table(
        {
            "experiment_id": pa.array(records["experiment_id"], type=pa.string()),
            "variant_id": pa.array(records["variant_id"], type=pa.string()),
            "content_id": pa.array(records["content_id"], type=pa.string()),
            "watch_time_seconds": pa.array(records["watch_time_seconds"], type=pa.float64()),
            "view_count": pa.array(records["view_count"], type=pa.int64()),
            "unique_viewers": pa.array(records["unique_viewers"], type=pa.int64()),
            "computation_date": pa.array(records["computation_date"], type=pa.date32()),
        }
    )
    pq.write_table(table, os.path.join(path, "data.parquet"))
    print(f"  content_consumption: {len(records['experiment_id'])} rows")


def main() -> None:
    parser = argparse.ArgumentParser(description="Generate synthetic Delta Lake data for PGO profiling")
    parser.add_argument("--output", required=True, help="Output directory for parquet files")
    args = parser.parse_args()

    print(f"Generating synthetic Delta Lake data in {args.output}...")
    os.makedirs(args.output, exist_ok=True)

    random.seed(42)  # Reproducible data
    generate_metric_summaries(args.output)
    generate_interleaving_scores(args.output)
    generate_daily_treatment_effects(args.output)
    generate_content_consumption(args.output)

    print("Done.")


if __name__ == "__main__":
    main()
