#!/system/bin/sh
until [ "$(getprop sys.boot_completed)" = "1" ]; do
  sleep 2
done
sh /data/adb/singboard/singboard.sh start
