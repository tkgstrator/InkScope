#!/bin/bash
# Build InkScope in debug mode and keep the live capture on the newest
# successfully built source. Each restart writes a new private PCAP.

set -u

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)" || exit 1
cd "$repo_dir" || exit 1

case $# in
    0) ;;
    1)
        if [[ $1 == "--build-only" ]]; then
            exec cargo build --locked -p inkscope
        fi
        printf 'usage: %s [--build-only | --mode normal|debug|salmon]\n' "$0" >&2
        exit 2
        ;;
    2)
        if [[ $1 != "--mode" ]]; then
            printf 'usage: %s [--build-only | --mode normal|debug|salmon]\n' "$0" >&2
            exit 2
        fi
        ;;
    *)
        printf 'usage: %s [--build-only | --mode normal|debug|salmon]\n' "$0" >&2
        exit 2
        ;;
esac

umask 077
interface="${INKSCOPE_INTERFACE:-en18}"
peer="${INKSCOPE_PEER:-192.168.1.180}"
mode="${INKSCOPE_MODE:-normal}"
if [[ $# -eq 2 ]]; then
    mode="$2"
fi
capture_dir="${INKSCOPE_CAPTURE_DIR:-$HOME/captures}"
game_key_file="${INKSCOPE_GAME_KEY_FILE:-$HOME/.config/inkscope/game_key.bin}"
pia_key_file="$HOME/.config/inkscope/pia_key.bin"
visualizer_url="${INKSCOPE_VISUALIZER_URL:-ws://127.0.0.1:10252/api/ingest}"
visualizer_token="${INKSCOPE_VISUALIZER_TOKEN:-}"
control_token="${INKSCOPE_CONTROL_TOKEN:-}"
control_port="${INKSCOPE_CONTROL_PORT:-10254}"

# The sibling Visualizer's private dotenv is the local default. Read only the
# requested token, without executing the dotenv or printing the secret.
visualizer_env="$repo_dir/../InkScope-Visualizer/.env.local"
read_visualizer_token() {
    python3 - "$visualizer_env" "$1" <<'PY'
import os
from pathlib import Path
import stat
import sys

path = Path(sys.argv[1])
name = sys.argv[2]
metadata = path.stat()
if metadata.st_uid != os.getuid() or stat.S_IMODE(metadata.st_mode) & 0o077:
    sys.exit("Visualizer .env.local must be owned by this user and mode 0600")
for line in path.read_text().splitlines():
    if line.startswith(name + "="):
        value = line.split("=", 1)[1].strip()
        if len(value) >= 2 and value[0] == value[-1] and value[0] in ('"', "'"):
            value = value[1:-1]
        print(value, end="")
        break
PY
}
if [[ -f $visualizer_env ]]; then
    if [[ -z $visualizer_token ]]; then
        visualizer_token="$(read_visualizer_token INKSCOPE_VISUALIZER_TOKEN)" || exit 1
    fi
    if [[ -z $control_token ]]; then
        control_token="$(read_visualizer_token INKSCOPE_CONTROL_TOKEN)" || exit 1
    fi
fi

visualizer_args=()
if [[ -n $visualizer_token ]]; then
    visualizer_args=(--visualizer-url "$visualizer_url")
    printf 'InkScope Visualizer export: %s\n' "$visualizer_url" >&2
else
    printf 'InkScope Visualizer export unavailable: set INKSCOPE_VISUALIZER_TOKEN or configure the sibling Visualizer .env.local\n' >&2
fi
control_args=()
if [[ -n $control_token ]]; then
    control_args=(--control-port "$control_port")
    printf 'InkScope control listener: 127.0.0.1:%s\n' "$control_port" >&2
fi

if [[ $mode != normal && $mode != debug && $mode != salmon ]]; then
    printf 'INKSCOPE_MODE must be normal, debug, or salmon\n' >&2
    exit 2
fi
printf 'InkScope capture mode: %s\n' "$mode" >&2
if [[ ! -r $game_key_file ]]; then
    printf 'GameKey file is not readable: %s\n' "$game_key_file" >&2
    exit 1
fi
mkdir -p "$capture_dir" || exit 1

capture_pid=""
capture_file=""
capture_started_at=0
candidate_pid=""
build_pid=""
capture_number=0
last_good_build=0
pending_capture=0
retry_delay=2
next_retry_at=0

stop_owned_process() {
    local pid="$1"
    if [[ -z $pid ]]; then
        return
    fi
    if kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || true
    fi
    wait "$pid" 2>/dev/null || true
}

cleanup() {
    trap - EXIT INT TERM
    stop_owned_process "$build_pid"
    stop_owned_process "$candidate_pid"
    stop_owned_process "$capture_pid"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

# Include file names, deletion, and nanosecond timestamps. A change during a
# build produces a different state and triggers another build before switching.
source_state() {
    python3 - "$repo_dir" <<'PY'
import hashlib
from pathlib import Path
import sys

root = Path(sys.argv[1])
files = [root / name for name in (
    "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain",
    "rust-toolchain.toml", ".cargo/config", ".cargo/config.toml",
)]
# Rust source may use include_bytes! or a build script may read crate files.
# Metadata-only scanning keeps the one-second poll cheap even for large assets.
files.extend(path for path in (root / "crates").rglob("*") if path.is_file())
state = hashlib.sha256()
for path in sorted(path for path in files if path.is_file()):
    stat = path.stat()
    state.update(str(path.relative_to(root)).encode())
    state.update(b"\0")
    state.update(f"{stat.st_mtime_ns}:{stat.st_ctime_ns}:{stat.st_size}:{stat.st_ino}".encode())
    state.update(b"\0")
print(state.hexdigest())
PY
}

discard_empty_pcap() {
    local path="$1" size
    [[ -n $path && -f $path ]] || return
    size="$(wc -c < "$path" 2>/dev/null || printf '0')"
    if (( size < 24 )) || { (( size == 24 )) && [[ $mode != salmon ]]; }; then
        rm -f -- "$path"
    fi
}

schedule_retry() {
    local now
    now="$(date +%s)"
    next_retry_at=$((now + retry_delay))
    printf 'Capture retry in %s seconds using the last successful build.\n' "$retry_delay" >&2
    retry_delay=$((retry_delay * 2))
    if (( retry_delay > 30 )); then
        retry_delay=30
    fi
}

start_capture() {
    capture_number=$((capture_number + 1))
    local stamp output size attempt old_pid
    stamp="$(date '+%Y%m%d-%H%M%S')"
    output="$capture_dir/inkscope-player-${stamp}-watch-$$-${capture_number}.pcap"
    printf 'Starting new InkScope capture: %s\n' "$output" >&2
    INKSCOPE_VISUALIZER_TOKEN="$visualizer_token" INKSCOPE_CONTROL_TOKEN="$control_token" \
        "$repo_dir/target/debug/inkscope" capture --interface "$interface" \
        --output "$output" --game-key-file "$game_key_file" \
        --pia-key-file "$pia_key_file" --peer "$peer" --mode "$mode" \
        "${visualizer_args[@]}" "${control_args[@]}" &
    candidate_pid=$!

    # The PCAP header proves that tcpdump opened the new capture stream. Keep
    # the prior capture if the new process exits or never writes its header.
    for ((attempt = 0; attempt < 30; attempt++)); do
        if ! kill -0 "$candidate_pid" 2>/dev/null; then
            wait "$candidate_pid" 2>/dev/null || true
            candidate_pid=""
            discard_empty_pcap "$output"
            printf 'New capture exited; any previous capture remains unchanged.\n' >&2
            return 1
        fi
        if [[ -f $output ]]; then
            size="$(wc -c < "$output" 2>/dev/null || printf '0')"
            if (( size >= 24 )); then
                sleep 0.2
                if ! kill -0 "$candidate_pid" 2>/dev/null; then
                    wait "$candidate_pid" 2>/dev/null || true
                    candidate_pid=""
                    discard_empty_pcap "$output"
                    printf 'New capture exited after writing its header; any previous capture remains unchanged.\n' >&2
                    return 1
                fi
                old_pid="$capture_pid"
                capture_pid="$candidate_pid"
                capture_file="$output"
                capture_started_at="$(date +%s)"
                candidate_pid=""
                if [[ -n $old_pid ]]; then
                    stop_owned_process "$old_pid"
                fi
                printf 'Using latest successful build; PCAP: %s\n' "$output" >&2
                return 0
            fi
        fi
        sleep 0.2
    done
    stop_owned_process "$candidate_pid"
    candidate_pid=""
    discard_empty_pcap "$output"
    printf 'New capture did not write a PCAP header; any previous capture remains unchanged.\n' >&2
    return 1
}

last_attempt_state=""
while :; do
    if [[ -n $capture_pid ]] && ! kill -0 "$capture_pid" 2>/dev/null; then
        wait "$capture_pid" 2>/dev/null || true
        capture_pid=""
        discard_empty_pcap "$capture_file"
        capture_file=""
        now="$(date +%s)"
        if (( now - capture_started_at >= 10 )); then
            retry_delay=2
        fi
        pending_capture=1
        if (( next_retry_at == 0 )); then
            schedule_retry
        fi
    fi
    current_state="$(source_state)" || exit 1
    if [[ $current_state != "$last_attempt_state" ]]; then
        last_attempt_state="$current_state"
        printf 'Building changed InkScope sources...\n' >&2
        cargo build --locked -p inkscope &
        build_pid=$!
        wait "$build_pid"
        build_status=$?
        build_pid=""
        after_build_state="$(source_state)" || exit 1
        if [[ $after_build_state != "$current_state" ]]; then
            printf 'Sources changed during the build; building again.\n' >&2
            continue
        fi
        if (( build_status == 0 )); then
            last_good_build=1
            pending_capture=1
            retry_delay=2
            next_retry_at=0
            if start_capture; then
                pending_capture=0
            else
                schedule_retry
            fi
        else
            printf 'Build failed; keeping the previous capture. Watching for the next edit.\n' >&2
        fi
    fi
    if (( pending_capture && last_good_build )); then
        now="$(date +%s)"
        if (( now >= next_retry_at )); then
            if start_capture; then
                pending_capture=0
                next_retry_at=0
            else
                schedule_retry
            fi
        fi
    fi
    sleep 1
done
