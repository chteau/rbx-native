#!/bin/sh
# Root helper for the macOS network shaper. Started once through osascript as
#   sh helper.sh DIR PARENT_PID
# and driven over two FIFOs in DIR: `cmd` (parent -> helper) and `status`
# (helper -> parent). Lines accepted on `cmd`:
#   out L J P | in L J P   latency us, jitter us, loss ppm
#   ports p1 ... pn        UDP ports to shape (0..64)
#   stop
# Only digits are accepted; nothing read is executed.
set -f
# System tools first; the rest of PATH only matters to the tests' stubs.
PATH="/sbin:/usr/sbin:/bin:/usr/bin:$PATH"

DIR=$1
PARENT=$2
case "$DIR" in ""|*[!A-Za-z0-9/._-]*) exit 2 ;; esac
case "$PARENT" in ""|*[!0-9]*) exit 2 ;; esac

ANCHOR=com.apple/rbxnative
IN_PIPES="4201 4202 4203 4204"
OUT_PIPES="4211 4212 4213 4214"
K=4
TOKEN=""
WATCH=""
PORTS=""

exec 3<"$DIR/cmd"
exec 4>"$DIR/status"

cleanup_rules() {
    pfctl -a "$ANCHOR" -F all >/dev/null 2>&1
    for n in $IN_PIPES $OUT_PIPES; do
        dnctl pipe delete "$n" >/dev/null 2>&1
    done
}

finish() {
    trap - EXIT
    [ -n "$WATCH" ] && kill "$WATCH" >/dev/null 2>&1
    cleanup_rules
    [ -n "$TOKEN" ] && pfctl -X "$TOKEN" >/dev/null 2>&1
}
trap finish EXIT
trap 'exit 0' INT TERM HUP

num() {
    case "$1" in
        ""|*[!0-9]*) return 1 ;;
        0) return 0 ;;
        0*) return 1 ;;
    esac
    [ ${#1} -le 7 ]
}

nth() {
    n=$1
    shift
    i=0
    for p in "$@"; do
        if [ "$i" -eq "$n" ]; then
            echo "$p"
            return
        fi
        i=$((i + 1))
    done
}

gen_rules() {
    for dir in out in; do
        if [ "$dir" = out ]; then list=$OUT_PIPES; else list=$IN_PIPES; fi
        i=0
        while [ "$i" -lt "$K" ]; do
            p=$(nth "$i" $list)
            prob=""
            if [ "$i" -lt $((K - 1)) ]; then
                prob=" probability $((100 / (K - i)))%"
            fi
            if [ "$dir" = out ]; then
                printf 'dummynet out quick proto udp from any port { %s } to any%s pipe %s\n' "$PORTS" "$prob" "$p"
            else
                printf 'dummynet in quick proto udp from any to any port { %s }%s pipe %s\n' "$PORTS" "$prob" "$p"
            fi
            i=$((i + 1))
        done
    done
}

apply_way() {
    verb=$1
    list=$2
    L=$3
    J=$4
    P=$5
    plr=$(printf '0.%06d' "$P")
    i=0
    ok=1
    while [ "$i" -lt "$K" ]; do
        p=$(nth "$i" $list)
        if [ "$K" -eq 1 ]; then
            d=$(((L + J / 2 + 500) / 1000))
        else
            d=$(((L + J * (2 * i + 1) / (2 * K) + 500) / 1000))
        fi
        dnctl pipe "$p" config delay "$d" plr "$plr" >/dev/null 2>&1 || ok=0
        i=$((i + 1))
    done
    if [ "$ok" -eq 1 ]; then echo "ok $verb" >&4; else echo "error $verb dnctl" >&4; fi
}

cleanup_rules
TOKEN=$(pfctl -E 2>&1 | sed -n 's/^Token : *\([0-9][0-9]*\).*/\1/p' | head -n 1)

if ! printf 'dummynet out quick proto udp from any to any probability 50%% pipe 4211\n' | pfctl -n -a "$ANCHOR" -f - >/dev/null 2>&1; then
    K=1
    echo nojitter >&4
fi

for n in $IN_PIPES $OUT_PIPES; do
    if ! dnctl pipe "$n" config delay 0 plr 0 >/dev/null 2>&1; then
        echo "error start dnctl" >&4
        exit 1
    fi
done

( while kill -0 "$PARENT" 2>/dev/null; do sleep 1; done; kill -TERM $$ ) >/dev/null 2>&1 &
WATCH=$!
echo "ready $$" >&4

while read -r verb rest <&3; do
    set -- $rest
    case "$verb" in
        out|in)
            if [ $# -eq 3 ] && num "$1" && num "$2" && num "$3" \
                && [ "$1" -le 1000000 ] && [ "$2" -le 1000000 ] && [ "$3" -le 5000 ]; then
                if [ "$verb" = out ]; then list=$OUT_PIPES; else list=$IN_PIPES; fi
                apply_way "$verb" "$list" "$1" "$2" "$3"
            else
                echo "error $verb malformed" >&4
            fi
            ;;
        ports)
            good=1
            [ $# -le 64 ] || good=0
            for p in "$@"; do
                num "$p" && [ "$p" -ge 1 ] && [ "$p" -le 65535 ] || good=0
            done
            if [ "$good" -eq 0 ]; then
                echo "error ports malformed" >&4
            else
                PORTS="$*"
                if [ $# -eq 0 ]; then
                    pfctl -a "$ANCHOR" -F rules >/dev/null 2>&1
                    r=$?
                else
                    gen_rules | pfctl -a "$ANCHOR" -f - >/dev/null 2>&1
                    r=$?
                fi
                if [ "$r" -eq 0 ]; then echo "ok ports" >&4; else echo "error ports pfctl" >&4; fi
            fi
            ;;
        stop)
            exit 0
            ;;
        *)
            echo "error unknown" >&4
            ;;
    esac
done
exit 0
