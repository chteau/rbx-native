# Runs as the first process of the shaper's user + network namespace,
# as uid 1000 with the namespace's capabilities, so `ip` and `tc` work.
# Prints `ns` once inside (slirp4netns may join now), `ready` once tap0 is
# up and shaped (or `error <step>`), then takes `<out|in> <delay_us>
# <spread_us> <loss_hundredths_percent>` lines on stdin and answers `ok` or
# `error ...` to each. Only digits are accepted; nothing read is executed.
exec 2>&1
echo ns
tries=0
until ip -o -4 addr show dev tap0 2>/dev/null | grep -q inet; do
  tries=$((tries + 1))
  if [ "$tries" -gt 200 ]; then echo "error tap0 never came up"; exit 1; fi
  sleep 0.05
done
step() { name=$1; shift; "$@" || { echo "error $name"; exit 1; }; }
# UDP only: the prio qdisc sends every packet to band 2 (1:2) except UDP,
# which the filter moves to band 3 (1:3), where netem sits.
step prio tc qdisc add dev tap0 root handle 1: prio bands 3 priomap 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1
step netem tc qdisc add dev tap0 parent 1:3 handle 30: netem delay 0us
step filter tc filter add dev tap0 parent 1: protocol ip u32 match ip protocol 17 0xff flowid 1:3
# Inbound: incoming UDP is redirected to ifb0 and delayed on its way out.
step ifb ip link add ifb0 type ifb
step ifb ip link set ifb0 up
step ingress tc qdisc add dev tap0 handle ffff: ingress
step mirred tc filter add dev tap0 parent ffff: protocol ip u32 match ip protocol 17 0xff action mirred egress redirect dev ifb0
step netem tc qdisc add dev ifb0 root netem delay 0us
echo ready
while read -r dir delay spread loss rest; do
  case "$dir" in
    out) set -- dev tap0 parent 1:3 handle 30: ;;
    in) set -- dev ifb0 root ;;
    *) echo "error unknown direction"; continue ;;
  esac
  case "$delay:$spread:$loss:$rest" in
    *[!0-9:]* | :* | *::* | *[0-9]) echo "error malformed line"; continue ;;
  esac
  percent="$((loss / 100)).$(printf %02d $((loss % 100)))%"
  if tc qdisc change "$@" netem delay "${delay}us" "${spread}us" loss "$percent"; then
    echo ok
  else
    echo "error tc"
  fi
done
