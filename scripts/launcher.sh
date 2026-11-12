#!/bin/sh
# Stock starts automatically if the Rust process exits or the user selects it.
root=/mnt/data/toon-rust
if [ -x "$root/toon-rust" ] && [ ! -f "$root/stock-ui" ]; then
    echo 0 > /sys/class/graphics/fb0/blank
    echo 0 > /tmp/fb0_blank
    "$root/toon-rust" > /var/volatile/tmp/toon-rust.log 2>&1
    touch "$root/stock-ui"
fi
exec /usr/bin/startqt
