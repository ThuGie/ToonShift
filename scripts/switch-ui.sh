#!/bin/sh
set -eu

root=/mnt/data/toon-rust
launcher="$root/launcher.sh"
rust_entry="trst:245:respawn:$launcher"
stock_entry='qtqt:245:respawn:/usr/bin/startqt >/dev/null 2>&1'
current=$(grep -E '^(qtqt|trst):' /etc/inittab || :)

replace_entry() {
    awk -F: -v id="$1" -v entry="$2" '$1 == id { print entry; next } { print }' \
        /etc/inittab > /etc/inittab.toon-rust
    chmod 644 /etc/inittab.toon-rust
    mv /etc/inittab.toon-rust /etc/inittab
}

case "${1:-}" in
    rust)
        "$root/toon-rust" --probe
        test -x "$launcher"
        if [ "$current" = "$stock_entry" ]; then
            if [ ! -e "$root/inittab.original" ]; then
                cp -p /etc/inittab "$root/inittab.original"
            fi
            replace_entry qtqt "$rust_entry"
        elif [ "$current" != "$rust_entry" ]; then
            echo "Unrecognized UI entry in /etc/inittab" >&2
            exit 1
        fi
        rm -f "$root/stock-ui"
        if [ "$current" = "$rust_entry" ]; then
            killall qt-gui qt-tenant-selection 2>/dev/null || :
        fi
        ;;
    stock)
        if [ "$current" = "$rust_entry" ]; then
            replace_entry trst "$stock_entry"
        elif [ "$current" != "$stock_entry" ]; then
            echo "Unrecognized UI entry in /etc/inittab" >&2
            exit 1
        fi
        ;;
    *) echo "Usage: $0 rust|stock" >&2; exit 2 ;;
esac

sync
/sbin/init q
