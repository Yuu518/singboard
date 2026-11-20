#!/system/bin/sh
(
  SERVICE_HOME=/data/adb/singboard
  SCRIPT=$SERVICE_HOME/singboard.sh
  ENV_FILE=$SERVICE_HOME/service.env

  until [ "$(getprop sys.boot_completed)" = "1" ]; do
    sleep 2
  done

  [ -f "$ENV_FILE" ] && . "$ENV_FILE"
  WORKDIR=${WORKDIR:-$SERVICE_HOME}
  CORE=${CORE:-$WORKDIR/sing-box}
  CONFIG=${CONFIG:-$WORKDIR/config.json}

  i=0
  while [ "$i" -lt 180 ]; do
    [ -f "$CORE" ] && [ -f "$CONFIG" ] && break
    sleep 2
    i=$((i + 1))
  done

  attempt=0
  while [ "$attempt" -lt 5 ]; do
    if sh "$SCRIPT" start; then
      sleep 3
      case "$(sh "$SCRIPT" status)" in
        running\ *) exit 0 ;;
      esac
    fi
    attempt=$((attempt + 1))
    sleep 5
  done
) >/dev/null 2>&1 &
