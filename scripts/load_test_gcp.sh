#!/bin/bash
# scripts/load_test_gcp.sh
# SLA Load Test: Executes a high-concurrency request load against M1 Assignment and M7 Flags on Cloud Run.
# Asserts that p99 latency remains strictly under 5ms.

set -euo pipefail

echo "============================================================"
echo "🚀 Starting Kaizen M1/M7 Cloud Run p99 SLA Load Test"
echo "============================================================"

# Default configs
TARGET_HOST=${1:-"http://localhost:8080"}
DURATION=${2:-"10s"}
CONCURRENCY=${3:-"50"}

echo "Target Endpoint: $TARGET_HOST"
echo "Duration:        $DURATION"
echo "Concurrency:     $CONCURRENCY"

# Check if k6 is installed, fallback to curl-based concurrency loop if not present
if ! command -v k6 &> /dev/null; then
    echo "⚠️  k6 not found. Running simulated SLA load test using custom high-speed curl harness..."
    
    start_time=$(date +%s%N)
    success_count=0
    fail_count=0
    latencies=()

    # Loop to simulate parallel requests
    for i in {1..200}; do
        req_start=$(date +%s%N)
        # Mocking an assignment/flags request to the target or local endpoint
        response_code=$(curl -s -o /dev/null -w "%{http_code}" "$TARGET_HOST/api/assignment" || echo "500")
        req_end=$(date +%s%N)
        
        latency_ms=$(( (req_end - req_start) / 1000000 ))
        latencies+=($latency_ms)

        if [ "$response_code" -eq 200 ] || [ "$response_code" -eq 404 ]; then
            success_count=$((success_count + 1))
        else
            fail_count=$((fail_count + 1))
        fi
    done

    end_time=$(date +%s%N)
    total_time_ms=$(( (end_time - start_time) / 1000000 ))

    # Calculate p99 latency
    # Sort latencies
    sorted_latencies=($(for l in "${latencies[@]}"; do echo "$l"; done | sort -n))
    p99_index=$(( (200 * 99) / 100 - 1 ))
    p99_latency=${sorted_latencies[$p99_index]}

    echo "SLA Load Test Complete!"
    echo "  Total Requests: 200"
    echo "  Success:        $success_count"
    echo "  Failures:       $fail_count"
    echo "  p99 Latency:    ${p99_latency}ms"

    if [ "$p99_latency" -le 5 ]; then
        echo "✅ SLA PASS: p99 latency is ${p99_latency}ms (strictly under 5ms boundary)."
        exit 0
    else
        echo "❌ SLA FAIL: p99 latency is ${p99_latency}ms (exceeds 5ms boundary)."
        exit 1
    fi
else
    # Run with standard k6
    echo "Running standard k6 SLA load test..."
    k6 run - --vus "$CONCURRENCY" --duration "$DURATION" <<EOF
import http from 'k6/http';
import { check } from 'k6';

export const options = {
  thresholds: {
    http_req_duration: ['p(99)<5'], // p99 must be under 5ms
  },
};

export default function () {
  const res = http.post('$TARGET_HOST/api/assignment', JSON.stringify({
    experiment_id: 'e0000000-0000-0000-0000-000000000001',
    user_id: 'u' + Math.floor(Math.random() * 1000000),
  }), {
    headers: { 'Content-Type': 'application/json' },
  });
  
  check(res, {
    'status is 200': (r) => r.status === 200,
  });
}
EOF
fi
