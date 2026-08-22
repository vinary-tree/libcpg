#!/usr/bin/env bash
set -euo pipefail

repository_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
workload=${1:-all}
profile_cpus=${LIBCPG_PROFILE_CPUS:-0,1}
repetitions=${LIBCPG_PROFILE_REPETITIONS:-1}
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
artifact_directory=${LIBCPG_PROFILE_OUTPUT_DIR:-target/verification/profile-$timestamp}
language_features=lang-rust,lang-python,lang-javascript,lang-go,lang-c

case "$workload" in
    cpg_construction | traversal_primitives | dfg_extraction | pdg_construction | \
        scc_analysis | dominator_analysis | pattern_vf2 | parallel_analysis_serial | \
        parallel_analysis_parallel | similarity_wl | analyze_corpus | all) ;;
    *)
        echo "unknown workload: $workload" >&2
        echo "expected cpg_construction, traversal_primitives, dfg_extraction," >&2
        echo "pdg_construction, scc_analysis, dominator_analysis, pattern_vf2," >&2
        echo "parallel_analysis_serial, parallel_analysis_parallel," >&2
        echo "similarity_wl," >&2
        echo "analyze_corpus, or all" >&2
        exit 2
        ;;
esac

cd "$repository_root"
mkdir -p "$artifact_directory"
cargo build --release --example profile_workloads --features "$language_features" \
    > "$artifact_directory/build.stdout" 2> "$artifact_directory/build.stderr"
profile_binary=target/release/examples/profile_workloads

echo "workload: $workload"
echo "repetitions: $repetitions"
echo "CPU affinity: $profile_cpus"
echo "artifacts: $artifact_directory"
if compgen -G '/sys/devices/system/cpu/cpu*/cpufreq/scaling_governor' > /dev/null; then
    governors=$(cat /sys/devices/system/cpu/cpu*/cpufreq/scaling_governor | sort -u | tr '\n' ' ')
else
    governors="unavailable"
fi
echo "governors: $governors"

/usr/bin/time -v -o "$artifact_directory/rss.txt" \
    env LIBCPG_PROFILE_REPETITIONS="$repetitions" \
    taskset -c "$profile_cpus" "$profile_binary" "$workload" \
    > "$artifact_directory/workload.stdout" 2> "$artifact_directory/workload.stderr"

perf stat -r 3 \
    -e cycles,instructions,cache-references,cache-misses,page-faults,dTLB-loads,dTLB-load-misses,LLC-loads,LLC-load-misses \
    -o "$artifact_directory/perf-stat.txt" -- \
    env LIBCPG_PROFILE_REPETITIONS="$repetitions" \
    taskset -c "$profile_cpus" "$profile_binary" "$workload" \
    > "$artifact_directory/perf-stat.stdout" 2> "$artifact_directory/perf-stat.stderr"

if perf record --call-graph lbr -F 997 -o "$artifact_directory/perf.data" -- \
    env LIBCPG_PROFILE_REPETITIONS="$repetitions" \
    taskset -c "$profile_cpus" "$profile_binary" "$workload" \
    > "$artifact_directory/perf-record-lbr.stdout" \
    2> "$artifact_directory/perf-record-lbr.stderr"; then
    echo "perf call graph: LBR"
elif perf record --call-graph dwarf -F 997 -o "$artifact_directory/perf.data" -- \
    env LIBCPG_PROFILE_REPETITIONS="$repetitions" \
    taskset -c "$profile_cpus" "$profile_binary" "$workload" \
    > "$artifact_directory/perf-record-dwarf.stdout" \
    2> "$artifact_directory/perf-record-dwarf.stderr"; then
    echo "perf call graph: DWARF fallback"
else
    echo "perf record unavailable; see perf-record-lbr.stderr and perf-record-dwarf.stderr" >&2
fi

if [[ -s "$artifact_directory/perf.data" ]]; then
    perf report --stdio --no-children -i "$artifact_directory/perf.data" \
        --sort overhead,symbol,dso --percent-limit 0.5 \
        > "$artifact_directory/perf-report.txt"
fi

env LIBCPG_PROFILE_REPETITIONS="$repetitions" taskset -c "$profile_cpus" \
    valgrind --tool=dhat --dhat-out-file="$artifact_directory/dhat.out" \
    --log-file="$artifact_directory/dhat.log" "$profile_binary" "$workload" \
    > "$artifact_directory/dhat.stdout" 2> "$artifact_directory/dhat.stderr"

env LIBCPG_PROFILE_REPETITIONS="$repetitions" taskset -c "$profile_cpus" \
    valgrind --tool=massif --pages-as-heap=yes \
    --massif-out-file="$artifact_directory/massif.out" \
    --log-file="$artifact_directory/massif.log" "$profile_binary" "$workload" \
    > "$artifact_directory/massif.stdout" 2> "$artifact_directory/massif.stderr"
ms_print "$artifact_directory/massif.out" > "$artifact_directory/massif-report.txt"

if command -v vtune > /dev/null; then
    if vtune -collect hotspots -result-dir "$artifact_directory/vtune-hotspots" -quiet -- \
        env LIBCPG_PROFILE_REPETITIONS="$repetitions" \
        taskset -c "$profile_cpus" "$profile_binary" "$workload" \
        > "$artifact_directory/vtune-hotspots.stdout" \
        2> "$artifact_directory/vtune-hotspots.stderr"; then
        vtune -report hotspots -result-dir "$artifact_directory/vtune-hotspots" \
            -format csv -report-output "$artifact_directory/vtune-hotspots.csv"
    else
        echo "VTune Hotspots unavailable; see vtune-hotspots.stderr" >&2
    fi

    if vtune -collect memory-access -result-dir "$artifact_directory/vtune-memory" -quiet -- \
        env LIBCPG_PROFILE_REPETITIONS="$repetitions" \
        taskset -c "$profile_cpus" "$profile_binary" "$workload" \
        > "$artifact_directory/vtune-memory.stdout" \
        2> "$artifact_directory/vtune-memory.stderr"; then
        vtune -report summary -result-dir "$artifact_directory/vtune-memory" \
            -format csv -report-output "$artifact_directory/vtune-memory.csv"
    else
        echo "VTune Memory Access unavailable; see vtune-memory.stderr" >&2
    fi
else
    echo "VTune unavailable on PATH" >&2
fi

echo "headless profile capture complete: $artifact_directory"
