#!/usr/bin/env python3
"""Generate synthetic experiment events for local testing.

Produces realistic exposure, metric, reward, and QoE events in JSON format
suitable for use with grpcurl against the M2 EventIngestionService.

Usage:
    # Generate 1000 mixed events to stdout (JSON Lines)
    python scripts/generate_synthetic_events.py --count 1000

    # Generate only exposure events
    python scripts/generate_synthetic_events.py --type exposure --count 500

    # Generate events for a specific experiment
    python scripts/generate_synthetic_events.py --experiment-id exp-cdn-test-001

    # Send events to the pipeline via grpcurl (requires grpcurl + running pipeline)
    python scripts/generate_synthetic_events.py --type exposure --count 1 --grpcurl |
        xargs -I{} grpcurl -plaintext -d '{}' localhost:50051 \\
        experimentation.pipeline.v1.EventIngestionService/IngestExposure

    # Write events to a JSONL file for batch loading
    python scripts/generate_synthetic_events.py --count 10000 --output events.jsonl

    # Generate interleaving experiment events with provenance
    python scripts/generate_synthetic_events.py --type exposure --interleaving --count 100
"""

import argparse
import json
import random
import sys
import uuid
from datetime import datetime, timezone, timedelta
from typing import Optional


# --- Realistic distributions ---

EXPERIMENT_IDS = [
    "exp-homepage-rank-001",
    "exp-cdn-latency-002",
    "exp-abr-algo-003",
    "exp-encoding-profile-004",
    "exp-search-relevance-005",
]

VARIANT_IDS = ["control", "treatment-a", "treatment-b"]

CDN_PROVIDERS = ["cloudflare", "akamai", "fastly", "cloudfront"]
ABR_ALGORITHMS = ["dash.js-default", "hls.js-abr", "custom-bola", "custom-mpc"]
ENCODING_PROFILES = ["h264-baseline", "h264-high", "h265-medium", "av1-low"]
PLATFORMS = ["ios", "android", "web", "smart-tv", "roku"]

METRIC_EVENT_TYPES = [
    "play_start",
    "watch_complete",
    "search",
    "add_to_list",
    "share",
    "skip_intro",
    "browse",
    "download",
    "rating",
]

ARM_IDS = ["arm-a", "arm-b", "arm-c", "arm-d"]

# Peak resolutions weighted by real-world distribution
PEAK_RESOLUTIONS = [360, 480, 720, 1080, 1440, 2160, 4320]
PEAK_RES_WEIGHTS = [0.02, 0.05, 0.20, 0.50, 0.10, 0.12, 0.01]

CONTENT_IDS = [f"content-{i:04d}" for i in range(1, 201)]  # 200 content items


def timestamp_now_iso() -> str:
    """Current time as ISO 8601 string (proto Timestamp format)."""
    now = datetime.now(timezone.utc)
    # Small jitter: ±5 minutes
    jitter = timedelta(seconds=random.randint(-300, 300))
    ts = now + jitter
    return ts.strftime("%Y-%m-%dT%H:%M:%S.%f")[:-3] + "Z"


def gen_exposure(
    experiment_id: Optional[str] = None,
    interleaving: bool = False,
) -> dict:
    """Generate a realistic ExposureEvent."""
    exp_id = experiment_id or random.choice(EXPERIMENT_IDS)
    user_id = f"user-{random.randint(1, 100000):06d}"

    event = {
        "event_id": str(uuid.uuid4()),
        "experiment_id": exp_id,
        "user_id": user_id,
        "variant_id": random.choice(VARIANT_IDS),
        "timestamp": timestamp_now_iso(),
        "platform": random.choice(PLATFORMS),
        "assignment_probability": round(random.uniform(0.01, 1.0), 4),
    }

    # 30% of exposures have session_id (session-level experiments)
    if random.random() < 0.3:
        event["session_id"] = str(uuid.uuid4())

    # Interleaving provenance
    if interleaving:
        num_items = random.randint(5, 20)
        provenance = {}
        for i in range(num_items):
            item_id = random.choice(CONTENT_IDS)
            algo_id = random.choice(["algo-a", "algo-b"])
            provenance[item_id] = algo_id
        event["interleaving_provenance"] = provenance

    return event


def gen_metric_event(experiment_id: Optional[str] = None) -> dict:
    """Generate a realistic MetricEvent."""
    event_type = random.choice(METRIC_EVENT_TYPES)

    # Value distribution depends on event type
    if event_type == "watch_complete":
        value = round(random.uniform(0, 1), 4)  # completion rate
    elif event_type in ("play_start", "browse", "search"):
        value = 1.0  # count event
    elif event_type == "rating":
        value = round(random.uniform(1, 5), 1)
    else:
        value = round(random.uniform(0, 100), 2)

    event = {
        "event_id": str(uuid.uuid4()),
        "user_id": f"user-{random.randint(1, 100000):06d}",
        "event_type": event_type,
        "value": value,
        "content_id": random.choice(CONTENT_IDS),
        "timestamp": timestamp_now_iso(),
    }

    # 40% have session_id
    if random.random() < 0.4:
        event["session_id"] = str(uuid.uuid4())

    return event


def gen_reward_event(experiment_id: Optional[str] = None) -> dict:
    """Generate a realistic RewardEvent."""
    exp_id = experiment_id or random.choice(EXPERIMENT_IDS)

    return {
        "event_id": str(uuid.uuid4()),
        "experiment_id": exp_id,
        "user_id": f"user-{random.randint(1, 100000):06d}",
        "arm_id": random.choice(ARM_IDS),
        "reward": round(random.uniform(0, 1), 4),
        "timestamp": timestamp_now_iso(),
    }


def gen_playback_metrics() -> dict:
    """Generate realistic PlaybackMetrics with correlated fields."""
    # Startup failure: ~2% of sessions
    startup_failed = random.random() < 0.02
    if startup_failed:
        return {
            "time_to_first_frame_ms": 0,
            "rebuffer_count": 0,
            "rebuffer_ratio": 0.0,
            "avg_bitrate_kbps": 0,
            "resolution_switches": 0,
            "peak_resolution_height": 0,
            "startup_failure_rate": 1.0,
            "playback_duration_ms": 0,
        }

    # Normal playback — fields are correlated
    duration_ms = random.randint(10_000, 7_200_000)  # 10s to 2h
    ttff = random.randint(100, 8_000)  # 100ms to 8s

    # Higher bitrate → higher resolution (correlated)
    peak_res = random.choices(PEAK_RESOLUTIONS, weights=PEAK_RES_WEIGHTS, k=1)[0]
    if peak_res >= 2160:
        bitrate = random.randint(15_000, 50_000)
    elif peak_res >= 1080:
        bitrate = random.randint(3_000, 15_000)
    elif peak_res >= 720:
        bitrate = random.randint(1_500, 5_000)
    else:
        bitrate = random.randint(500, 2_000)

    # Longer sessions → more rebuffers (correlated)
    rebuffer_rate = random.uniform(0, 0.005)  # 0-0.5% of duration
    rebuffer_count = max(0, int(duration_ms / 60_000 * random.uniform(0, 2)))
    rebuffer_ratio = min(1.0, round(rebuffer_rate, 4))

    # Resolution switches correlate with session length
    switches = max(0, int(duration_ms / 120_000 * random.uniform(0, 3)))

    return {
        "time_to_first_frame_ms": ttff,
        "rebuffer_count": rebuffer_count,
        "rebuffer_ratio": rebuffer_ratio,
        "avg_bitrate_kbps": bitrate,
        "resolution_switches": switches,
        "peak_resolution_height": peak_res,
        "startup_failure_rate": 0.0,
        "playback_duration_ms": duration_ms,
    }


def gen_qoe_event(experiment_id: Optional[str] = None) -> dict:
    """Generate a realistic QoEEvent."""
    return {
        "event_id": str(uuid.uuid4()),
        "session_id": str(uuid.uuid4()),
        "content_id": random.choice(CONTENT_IDS),
        "user_id": f"user-{random.randint(1, 100000):06d}",
        "metrics": gen_playback_metrics(),
        "cdn_provider": random.choice(CDN_PROVIDERS),
        "abr_algorithm": random.choice(ABR_ALGORITHMS),
        "encoding_profile": random.choice(ENCODING_PROFILES),
        "timestamp": timestamp_now_iso(),
    }


# --- Event type mix weights (realistic SVOD platform) ---
EVENT_MIX = {
    "exposure": 0.20,
    "metric": 0.50,
    "reward": 0.10,
    "qoe": 0.20,
}


def gen_event(
    event_type: Optional[str] = None,
    experiment_id: Optional[str] = None,
    interleaving: bool = False,
) -> tuple[str, dict]:
    """Generate a single event, returning (type, event_dict)."""
    if event_type is None:
        event_type = random.choices(
            list(EVENT_MIX.keys()),
            weights=list(EVENT_MIX.values()),
            k=1,
        )[0]

    generators = {
        "exposure": lambda: gen_exposure(experiment_id, interleaving),
        "metric": lambda: gen_metric_event(experiment_id),
        "reward": lambda: gen_reward_event(experiment_id),
        "qoe": lambda: gen_qoe_event(experiment_id),
    }

    return event_type, generators[event_type]()


def wrap_for_grpcurl(event_type: str, event: dict) -> dict:
    """Wrap event in the gRPC request envelope."""
    return {"event": event}


def main():
    parser = argparse.ArgumentParser(
        description="Generate synthetic experiment events for testing"
    )
    parser.add_argument(
        "--count", type=int, default=100, help="Number of events to generate"
    )
    parser.add_argument(
        "--type",
        choices=["exposure", "metric", "reward", "qoe", "mixed"],
        default="mixed",
        help="Event type (default: mixed)",
    )
    parser.add_argument(
        "--experiment-id", type=str, help="Fix experiment_id for all events"
    )
    parser.add_argument(
        "--interleaving",
        action="store_true",
        help="Include interleaving provenance on exposure events",
    )
    parser.add_argument(
        "--grpcurl",
        action="store_true",
        help="Wrap events in gRPC request envelope",
    )
    parser.add_argument(
        "--output", type=str, help="Output file (default: stdout)"
    )
    parser.add_argument(
        "--seed", type=int, help="Random seed for reproducibility"
    )

    args = parser.parse_args()

    if args.seed is not None:
        random.seed(args.seed)

    event_type = None if args.type == "mixed" else args.type
    out = open(args.output, "w") if args.output else sys.stdout

    try:
        for _ in range(args.count):
            etype, event = gen_event(event_type, args.experiment_id, args.interleaving)
            if args.grpcurl:
                event = wrap_for_grpcurl(etype, event)
            line = {"type": etype, **event} if not args.grpcurl else event
            out.write(json.dumps(line) + "\n")
    finally:
        if args.output:
            out.close()

    if args.output:
        print(f"Wrote {args.count} events to {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
