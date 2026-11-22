#!/system/bin/sh
DIR=${0%/*}
[ -f "$DIR/service.env" ] && . "$DIR/service.env"
WORKDIR=${WORKDIR:-$DIR}
CORE=${CORE:-$WORKDIR/sing-box}
CONFIG=${CONFIG:-$WORKDIR/config.json}
RUN_DIR=$DIR/run
PID_FILE=$RUN_DIR/sing-box.pid
LOG_DIR=$DIR/logs
LOG_FILE=$LOG_DIR/singboard.log
LOG_LIMIT=1048576
STOP_TIMEOUT=60

core_pid() {
  [ -f "$PID_FILE" ] || return 1
  pid=$(cat "$PID_FILE" 2>/dev/null)
  [ -n "$pid" ] && [ -d "/proc/$pid" ] || return 1
  case "$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null)" in
    "$CORE "*) echo "$pid" ;;
    *) return 1 ;;
  esac
}

trim_log() {
  [ -f "$LOG_FILE" ] || return 0
  size=$(stat -c %s "$LOG_FILE" 2>/dev/null || echo 0)
  if [ "$size" -gt "$LOG_LIMIT" ]; then
    tmp="$LOG_FILE.tmp.$$"
    if tail -c 262144 "$LOG_FILE" > "$tmp" 2>/dev/null; then
      mv -f "$tmp" "$LOG_FILE"
    else
      rm -f "$tmp"
    fi
  fi
  return 0
}

record() {
  mkdir -p "$LOG_DIR"
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" >> "$LOG_FILE"
}

fail() {
  record "$*"
  echo "$*"
  return 1
}

detach_cgroup() {
  for procs in /sys/fs/cgroup/cgroup.procs /acct/cgroup.procs /dev/cg2_bpf/cgroup.procs; do
    [ -w "$procs" ] && echo "$1" > "$procs" 2>/dev/null
  done
  return 0
}

start() {
  core_pid >/dev/null && return 0
  trim_log
  [ -f "$CORE" ] || { fail "sing-box core not found: $CORE"; return 1; }
  [ -f "$CONFIG" ] || { fail "config not found: $CONFIG"; return 1; }
  chmod 755 "$CORE"
  mkdir -p "$RUN_DIR" "$WORKDIR"
  record "starting $CORE run -c $CONFIG -D $WORKDIR"
  cd "$WORKDIR" || { fail "cannot enter working directory: $WORKDIR"; return 1; }
  if command -v setsid >/dev/null 2>&1; then
    setsid "$CORE" run -c "$CONFIG" -D "$WORKDIR" --disable-color </dev/null >>"$LOG_FILE" 2>&1 &
  elif command -v nohup >/dev/null 2>&1; then
    nohup "$CORE" run -c "$CONFIG" -D "$WORKDIR" --disable-color </dev/null >>"$LOG_FILE" 2>&1 &
  else
    "$CORE" run -c "$CONFIG" -D "$WORKDIR" --disable-color </dev/null >>"$LOG_FILE" 2>&1 &
  fi
  pid=$!
  echo "$pid" > "$PID_FILE"
  detach_cgroup "$pid"
  record "started pid $pid"
  return 0
}

signal() {
  pid=$(core_pid) || { rm -f "$PID_FILE"; return 0; }
  record "stopping pid $pid"
  kill -TERM "$pid" 2>/dev/null
  return 0
}

stop() {
  pid=$(core_pid) || { rm -f "$PID_FILE"; return 0; }
  signal
  i=0
  while [ -n "$(core_pid)" ]; do
    if [ "$i" -ge $((STOP_TIMEOUT * 10)) ]; then
      fail "sing-box (pid $pid) is still cleaning up after ${STOP_TIMEOUT}s; not killed to keep network rules consistent"
      return 1
    fi
    sleep 0.1
    i=$((i + 1))
  done
  rm -f "$PID_FILE"
  record "stopped pid $pid"
  return 0
}

status() {
  if pid=$(core_pid); then
    stat_line=$(cat "/proc/$pid/stat" 2>/dev/null)
    if [ -z "$stat_line" ]; then
      echo stopped
      return 0
    fi
    rest=${stat_line##*) }
    set -f
    set -- $rest
    set +f
    utime=${12}
    stime=${13}
    started=${20}
    if [ -z "$utime" ] || [ -z "$stime" ] || [ -z "$started" ]; then
      echo stopped
      return 0
    fi
    rss=$(grep VmRSS "/proc/$pid/status" 2>/dev/null | tr -s ' \t' ' ' | cut -d ' ' -f 2)
    cpus=$(grep -c '^processor' /proc/cpuinfo 2>/dev/null)
    echo "running $pid $started $(cut -d ' ' -f 1 /proc/uptime) $utime $stime ${rss:-0} ${cpus:-1}"
  else
    echo stopped
  fi
}

show_log() {
  [ -f "$LOG_FILE" ] || return 0
  line=$(grep -n '^\[[^]]*\] starting ' "$LOG_FILE" | tail -n 1 | cut -d : -f 1)
  tail -n +"${line:-1}" "$LOG_FILE" | tail -c 8192
  return 0
}

case "$1" in
  start) start ;;
  stop) stop ;;
  signal) signal ;;
  restart) stop && start ;;
  status) status ;;
  log) show_log ;;
  *) echo "usage: $0 {start|stop|signal|restart|status|log}"; exit 1 ;;
esac
