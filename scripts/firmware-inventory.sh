#!/bin/sh
# Run on the Toon. Outputs paths and metadata only.
set -eu

printf 'size_bytes\ttype\tpath\tlink_target\n'
find /qmf /etc /mnt/data/qmf/config /mnt/persist/etc -xdev \( -type f -o -type l \) \
    -exec sh -c '
        for path do
            case "$path" in
                */shadow|*/passwd|*/wpa_supplicant*|*/authorized_keys|*/dropbear*key|*/token*|*.key)
                    continue ;;
            esac
            metadata=$(stat -c "%s|%F" "$path") || continue
            target=
            if [ -L "$path" ]; then target=$(readlink "$path") || target=; fi
            printf "%s\t%s\t%s\t%s\n" "${metadata%%|*}" "${metadata#*|}" "$path" "$target"
        done
    ' sh {} +

printf '\npackage\tpath\n'
for list in /usr/lib/opkg/info/*.list /var/lib/opkg/info/*.list; do
    [ -f "$list" ] || continue
    package=${list##*/}
    package=${package%.list}
    awk -v package="$package" '/^\/(qmf|HCBv2|etc)\// && !/\/(shadow|passwd|authorized_keys)$|\.key$/ {print package "\t" $0}' "$list"
done
