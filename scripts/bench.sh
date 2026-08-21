#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 BENCH [criterion arguments...]" >&2
    exit 2
fi

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
benchmark_name=$1
shift
benchmark_cpus=${LIBCPG_BENCH_CPUS:-0,1}
benchmark_features=${LIBCPG_BENCH_FEATURES:-}
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
artifact_directory=${LIBCPG_BENCH_OUTPUT_DIR:-target/verification}
log_path="$artifact_directory/libcpg-${benchmark_name}-${timestamp}.log"

cd "$repository_root"
mkdir -p "$artifact_directory"
echo "CPU affinity: $benchmark_cpus"
echo "features: ${benchmark_features:-none}"
if compgen -G '/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor' > /dev/null; then
    governors=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort -u | tr '\n' ' ')
else
    governors="unavailable"
fi
echo "governors: $governors"
echo "capturing benchmark output at $log_path"

set -o pipefail
cargo_arguments=(bench --bench "$benchmark_name" --no-default-features)
if [[ -n "$benchmark_features" ]]; then
    cargo_arguments+=(--features "$benchmark_features")
fi
taskset -c "$benchmark_cpus" cargo "${cargo_arguments[@]}" -- "$@" 2>&1 | tee "$log_path"
