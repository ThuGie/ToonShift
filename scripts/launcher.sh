#!/bin/sh
# Stock starts automatically if the Rust process exits or the user selects it.
root=/mnt/data/toon-rust
if [ -x "$root/toon-rust" ] && [ ! -f "$root/stock-ui" ]; then
    echo 0 > /sys/class/graphics/fb0/blank
    echo 0 > /tmp/fb0_blank
    if [ -x "$root/update-supervisor" ]; then
        "$root/update-supervisor" --supervise > /var/volatile/tmp/toon-rust.log 2>&1 &
        supervisor=$!
        trap 'kill "$supervisor" 2>/dev/null || :; wait "$supervisor" 2>/dev/null || :; exit 0' INT TERM
        wait "$supervisor"
        trap - INT TERM
    else
        "$root/toon-rust" > /var/volatile/tmp/toon-rust.log 2>&1
    fi
    if [ -f /run/toonshift-temporary-stock ]; then
        rm -f /run/toonshift-temporary-stock
        /usr/bin/startqt &
        stock_pid=$!
        trap 'kill "$stock_pid" 2>/dev/null || :; exit 0' INT TERM
        "$root/toon-rust" --stock-session-return "$stock_pid" >> /var/volatile/tmp/toon-rust.log 2>&1
        kill "$stock_pid" 2>/dev/null || :
        wait "$stock_pid" 2>/dev/null || :
        exit 0
    fi
    touch "$root/stock-ui"
fi
exec /usr/bin/startqt
