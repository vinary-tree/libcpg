#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 ]]; then
    echo "usage: $0 BENCH [criterion arguments...]" >&2
    exit 2
fi

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
benchmark_name=$1
shift
benchmark_cpus=${LIBCPG_BENCH_CPUS:-2}
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
log_path="/tmp/libcpg-${benchmark_name}-${timestamp}.log"

cd "$repository_root"
echo "CPU affinity: $benchmark_cpus"
echo "governors: $(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor 2>/dev/null | sort -u | tr '\n' ' ' || true)"
echo "capturing benchmark output at $log_path"

set -o pipefail
taskset -c "$benchmark_cpus" cargo bench --bench "$benchmark_name" --no-default-features -- "$@" 2>&1 | tee "$log_path"
