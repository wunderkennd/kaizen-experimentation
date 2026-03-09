#!/usr/bin/env python3
"""
Synthetic Event Generator for M2 Pipeline Testing.

Generates realistic exposure, metric, reward, and QoE events for local
development, load testing, and integration testing with Agent-3 (metrics).

Usage:
    # Generate 1000 exposure events as JSON (for grpcurl)
    python scripts/generate_synthetic_events.py --type exposure --count 1000

    # Generate QoE events with correlated playback metrics
    python scripts/generate_synthetic_events.py --type qoe --count 500 --seed 42

    # Generate interleaving exposure events with provenance maps
    python scripts/generate_synthetic_events.py --type exposure --count 100 --interleaving

    # Pipe directly to grpcurl (one event at a time)
    python scripts/generate_synthetic_events.py --type exposure --count 10 --grpcurl | bash

    # Write to file
    python scripts/generate_synthetic_events.py --type metric --count 5000 --output events.jsonl

    # All event types (for full pipeline testing)
    python scripts/generate_synthetic_events.py --type all --count 200
"""

import argparse
import json
import math
import random
import sys
import uuid
from datetime import datetime, timezone, timedelta
from typing import Any

# ---------------------------------------------------------------------------
# Constants — realistic distributions for SVOD experiments
# ---------------------------------------------------------------------------

EXPERIMENT_IDS = [
    "homepage_recs_v2",
    "search_ranking_v3",
    "player_ui_experiment",
    "content_cold_start_bandit",
    "checkout_flow_v2",
    "personalization_v4",
    "onboarding_experiment",
    "trailer_autoplay_test",
    "subtitle_default_on",
    "browse_infinite_scroll",
]

VARIANT_IDS = {
    "homepage_recs_v2": ["control", "collaborative_filter", "deep_learning"],
    "search_ranking_v3": ["control", "bm25_boost", "neural_ranker"],
    "player_ui_experiment": ["control", "minimal_ui", "overlay_ui"],
    "content_cold_start_bandit": ["arm_0", "arm_1", "arm_2", "arm_3"],
    "checkout_flow_v2": ["control", "single_page"],
    "personalization_v4": ["control", "treatment"],
    "onboarding_experiment": ["control", "guided_tour", "video_intro"],
    "trailer_autoplay_test": ["control", "autoplay_on"],
    "subtitle_default_on": ["control", "subtitles_default"],
    "browse_infinite_scroll": ["control", "infinite_scroll"],
}

METRIC_IDS = [
    "watch_time_minutes",
    "sessions_per_day",
    "content_completion_rate",
    "search_result_clicks",
    "playback_start_rate",
    "error_rate",
    "rebuffer_ratio",
    "time_to_first_frame_ms",
    "revenue_per_user",
    "trial_conversion_rate",
    "churn_rate_7d",
    "engagement_score",
]

PLATFORMS = ["web", "ios", "android", "smart_tv", "roku", "fire_tv"]
COUNTRIES = ["US", "GB", "CA", "DE", "FR", "JP", "BR", "IN", "AU", "MX"]
DEVICE_TYPES = ["desktop", "mobile", "tablet", "tv", "console"]

CDN_PROVIDERS = ["cloudfront", "akamai", "fastly", "cloudflare"]
ABR_ALGORITHMS = ["buffer_based", "rate_based", "hybrid_abr", "low_latency"]
ENCODING_PROFILES = ["h264_baseline", "h264_high", "h265_main", "av1_main", "vp9"]

CONTENT_IDS = [f"content_{i:04d}" for i in range(1, 201)]

# Interleaving algorithms (for provenance maps)
INTERLEAVING_ALGORITHMS = ["team_draft_a", "team_draft_b", "balanced_interleave"]


# ---------------------------------------------------------------------------
# Event generators
# ---------------------------------------------------------------------------

def make_timestamp(rng: random.Random, jitter_hours: int = 12) -> dict:
    """Generate a protobuf Timestamp dict within ±jitter_hours of now."""
    now = datetime.now(timezone.utc)
    offset = timedelta(hours=rng.uniform(-jitter_hours, 0), minutes=rng.uniform(-30, 30))
    ts = now + offset
    return {
        "seconds": int(ts.timestamp()),
        "nanos": rng.randint(0, 999_999_999),
    }


def make_user_id(rng: random.Random) -> str:
    return f"user_{rng.randint(1, 1_000_000):07d}"


def make_session_id(rng: random.Random) -> str:
    return f"session_{uuid.UUID(int=rng.getrandbits(128)).hex[:16]}"


def generate_exposure(rng: random.Random, interleaving: bool = False) -> dict:
    """Generate a realistic ExposureEvent."""
    exp_id = rng.choice(EXPERIMENT_IDS)
    variants = VARIANT_IDS.get(exp_id, ["control", "treatment"])
    variant = rng.choice(variants)
    user_id = make_user_id(rng)

    event: dict[str, Any] = {
        "event_id": f"exp_{uuid.uuid4().hex[:20]}",
        "experiment_id": exp_id,
        "user_id": user_id,
        "variant_id": variant,
        "timestamp": make_timestamp(rng),
        "assignment_context": {
            "platform": rng.choice(PLATFORMS),
            "country": rng.choice(COUNTRIES),
            "device_type": rng.choice(DEVICE_TYPES),
            "session_id": make_session_id(rng),
        },
    }

    # Add interleaving provenance for some experiments
    if interleaving or (exp_id in ("homepage_recs_v2", "search_ranking_v3") and rng.random() < 0.3):
        num_items = rng.randint(5, 20)
        provenance = {}
        for i in range(num_items):
            item_id = rng.choice(CONTENT_IDS)
            algo = rng.choice(INTERLEAVING_ALGORITHMS)
            provenance[item_id] = algo
        event["interleaving_provenance"] = provenance

    return event


def generate_metric(rng: random.Random) -> dict:
    """Generate a realistic MetricEvent."""
    metric_id = rng.choice(METRIC_IDS)

    # Realistic value distributions per metric type
    value_generators = {
        "watch_time_minutes": lambda: max(0, rng.gauss(45, 30)),
        "sessions_per_day": lambda: max(0, rng.gauss(2.5, 1.5)),
        "content_completion_rate": lambda: min(1.0, max(0, rng.betavariate(2, 3))),
        "search_result_clicks": lambda: max(0, int(rng.gauss(3, 2))),
        "playback_start_rate": lambda: min(1.0, max(0, rng.betavariate(8, 2))),
        "error_rate": lambda: max(0, rng.expovariate(100)),  # low rate
        "rebuffer_ratio": lambda: max(0, min(1, rng.expovariate(50))),
        "time_to_first_frame_ms": lambda: max(100, rng.gauss(1500, 800)),
        "revenue_per_user": lambda: max(0, rng.gauss(12.99, 5)),
        "trial_conversion_rate": lambda: 1.0 if rng.random() < 0.15 else 0.0,
        "churn_rate_7d": lambda: 1.0 if rng.random() < 0.03 else 0.0,
        "engagement_score": lambda: min(100, max(0, rng.gauss(65, 20))),
    }

    value_fn = value_generators.get(metric_id, lambda: rng.uniform(0, 100))

    return {
        "event_id": f"met_{uuid.uuid4().hex[:20]}",
        "experiment_id": rng.choice(EXPERIMENT_IDS),
        "user_id": make_user_id(rng),
        "metric_id": metric_id,
        "metric_value": round(value_fn(), 6),
        "timestamp": make_timestamp(rng),
    }


def generate_reward(rng: random.Random) -> dict:
    """Generate a realistic RewardEvent (for bandit experiments)."""
    bandit_experiments = ["content_cold_start_bandit"]
    exp_id = rng.choice(bandit_experiments)
    variants = VARIANT_IDS[exp_id]

    # Reward is typically 0 or 1 (binary), or continuous [0, 1]
    if rng.random() < 0.7:
        reward = 1.0 if rng.random() < 0.3 else 0.0  # binary
    else:
        reward = round(rng.betavariate(2, 5), 4)  # continuous

    return {
        "event_id": f"rwd_{uuid.uuid4().hex[:20]}",
        "experiment_id": exp_id,
        "user_id": make_user_id(rng),
        "arm_id": rng.choice(variants),
        "reward_value": reward,
        "timestamp": make_timestamp(rng),
    }


def generate_qoe(rng: random.Random) -> dict:
    """Generate a realistic QoEEvent with correlated PlaybackMetrics."""
    # Base parameters (correlated)
    session_duration_ms = int(max(1000, rng.gauss(30 * 60 * 1000, 20 * 60 * 1000)))
    session_duration_ms = min(session_duration_ms, 4 * 3600 * 1000)  # cap at 4h

    # Bitrate correlates with resolution
    avg_bitrate = int(max(500, rng.gauss(5000, 3000)))
    avg_bitrate = min(avg_bitrate, 50000)

    if avg_bitrate > 15000:
        resolution = rng.choice([2160, 1440])
    elif avg_bitrate > 5000:
        resolution = rng.choice([1080, 720])
    elif avg_bitrate > 2000:
        resolution = rng.choice([720, 480])
    else:
        resolution = rng.choice([480, 360, 240])

    # Longer sessions → more rebuffers
    session_hours = session_duration_ms / 3_600_000
    rebuffer_count = int(max(0, rng.gauss(session_hours * 2, session_hours)))
    rebuffer_count = min(rebuffer_count, 500)

    # Rebuffer ratio
    if session_duration_ms > 0:
        rebuffer_ratio = min(1.0, max(0.0, rebuffer_count * rng.uniform(500, 3000) / session_duration_ms))
    else:
        rebuffer_ratio = 0.0

    # Resolution switches correlate with session duration
    resolution_switches = int(max(0, rng.gauss(session_hours * 3, session_hours * 2)))
    resolution_switches = min(resolution_switches, 200)

    # TTFF — shorter on fast connections
    ttff_base = 2000 if avg_bitrate > 10000 else 4000
    ttff = int(max(100, rng.gauss(ttff_base, ttff_base * 0.5)))
    ttff = min(ttff, 30000)

    # Startup failure (rare)
    startup_failure = 1.0 if rng.random() < 0.02 else 0.0

    playback_metrics = {
        "rebuffer_ratio": round(rebuffer_ratio, 6),
        "time_to_first_frame_ms": ttff,
        "rebuffer_count": rebuffer_count,
        "avg_bitrate_kbps": avg_bitrate,
        "peak_resolution_height": resolution,
        "resolution_switches": resolution_switches,
        "playback_duration_ms": session_duration_ms,
        "startup_failure_rate": startup_failure,
    }

    return {
        "event_id": f"qoe_{uuid.uuid4().hex[:20]}",
        "experiment_id": rng.choice(EXPERIMENT_IDS),
        "user_id": make_user_id(rng),
        "session_id": make_session_id(rng),
        "content_id": rng.choice(CONTENT_IDS),
        "cdn_provider": rng.choice(CDN_PROVIDERS),
        "abr_algorithm": rng.choice(ABR_ALGORITHMS),
        "encoding_profile": rng.choice(ENCODING_PROFILES),
        "playback_metrics": playback_metrics,
        "timestamp": make_timestamp(rng),
    }


# ---------------------------------------------------------------------------
# Output formatters
# ---------------------------------------------------------------------------

def format_grpcurl_cmd(event_type: str, event: dict, host: str = "localhost:50051") -> str:
    """Format as a grpcurl command."""
    service = "experimentation.pipeline.v1.EventIngestionService"
    rpc_map = {
        "exposure": "IngestExposure",
        "metric": "IngestMetricEvent",
        "reward": "IngestRewardEvent",
        "qoe": "IngestQoEEvent",
    }
    rpc = rpc_map[event_type]
    payload = json.dumps({"event": event}, separators=(",", ":"))
    return f"grpcurl -plaintext -d '{payload}' {host} {service}/{rpc}"


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(
        description="Generate synthetic events for M2 pipeline testing",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    parser.add_argument(
        "--type", "-t",
        choices=["exposure", "metric", "reward", "qoe", "all"],
        default="exposure",
        help="Event type to generate (default: exposure)",
    )
    parser.add_argument("--count", "-n", type=int, default=10, help="Number of events (default: 10)")
    parser.add_argument("--seed", "-s", type=int, default=None, help="Random seed for reproducibility")
    parser.add_argument("--interleaving", action="store_true", help="Add interleaving provenance to exposures")
    parser.add_argument("--grpcurl", action="store_true", help="Output as grpcurl commands")
    parser.add_argument("--host", default="localhost:50051", help="gRPC host for --grpcurl (default: localhost:50051)")
    parser.add_argument("--output", "-o", default=None, help="Output file (default: stdout)")
    parser.add_argument("--compact", action="store_true", help="Compact JSON output (no pretty printing)")

    args = parser.parse_args()

    rng = random.Random(args.seed)
    generators = {
        "exposure": lambda: generate_exposure(rng, args.interleaving),
        "metric": lambda: generate_metric(rng),
        "reward": lambda: generate_reward(rng),
        "qoe": lambda: generate_qoe(rng),
    }

    out = open(args.output, "w") if args.output else sys.stdout

    try:
        for i in range(args.count):
            if args.type == "all":
                # Rotate through event types
                event_types = ["exposure", "metric", "reward", "qoe"]
                event_type = event_types[i % len(event_types)]
            else:
                event_type = args.type

            event = generators[event_type]()

            if args.grpcurl:
                print(format_grpcurl_cmd(event_type, event, args.host), file=out)
            else:
                indent = None if args.compact else 2
                print(json.dumps({"type": event_type, "event": event}, indent=indent), file=out)
    finally:
        if args.output and out is not sys.stdout:
            out.close()

    if args.output:
        print(f"Wrote {args.count} events to {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
