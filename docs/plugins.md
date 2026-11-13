# ToonShift plugin repositories

The Store reads built-in widgets and local JSON files in `/mnt/data/toon-rust/catalog`. To add any number of sources within the catalog limits, sync each public GitHub repository independently. The same source filename is updated atomically, and other repositories keep their files.

```powershell
./scripts/sync-plugin-repo.ps1 -Toon toon -Repository ThuGie/ToonShift-plugins -Commit <full-commit-sha>
./scripts/sync-plugin-repo.ps1 -Toon toon -Repository another-owner/another-catalog -Commit <full-commit-sha>
```

From the touchscreen, **Store → Refresh official** downloads the official index. **Add GitHub source** accepts `owner/repository` for its main branch or `owner/repository@<full-commit-sha>` for a pinned revision. **Install** downloads only the selected package. ToonShift checks its SHA-256, ID, and release before activating it. **Update** replaces the package after those checks and preserves its tile and location. An installed package stays on its existing release until updated.

The PowerShell script is also available to download and transfer a pinned catalog over SSH. To stop using a source, remove its `owner--repository.json` over SSH. Remove any installed tiles in Store before removing the source if you want them removed from your dashboard too. A missing source displays its installed widgets as **Unavailable**.

```json
{
  "version": 1,
  "plugins": [
    {
      "id": "yourname-office-humidity",
      "title": "Office humidity",
      "description": "Relative humidity reported by a local collector",
      "source": "file"
    }
  ]
}
```

Use a globally unique lowercase ID with digits and hyphens, up to 40 characters. Valid sources are `clock`, `boiler`, `next_target`, `note` with `text`, and `file`. With `file`, a collector publishes one UTF-8 line to `/mnt/data/toon-rust/values/<id>.txt` using a temporary file and rename. Files must be at most 1,024 bytes. A catalog cannot add executable code, configure networking, or write thermostat settings.

Each version 1 catalog has room for 64 entries within 32 KiB. The app reads up to 64 catalog files and 128 total widgets. Built-in IDs take precedence, followed by catalog filenames in sorted order. Use a publisher prefix to avoid collisions.

For separate downloads, include `"artifact": {"path":"plugins/yourname-widget.json","sha256":"…"}` in the index entry. Use `"source":"file"` as the listing's placeholder. The package defines the real source, and shares the entry's `id` and `release`. The SHA-256 covers the exact UTF-8 bytes served by GitHub. Standalone and inline manifests remain supported.

The generic `https_json` source accepts an HTTPS URL template, a numeric JSON pointer, suffix and precision, and optional detail labels, range rows, locations, and location-search schema. Weather's Open-Meteo endpoints, field mappings, and English and Dutch condition labels are in its own package. ToonShift runs the same renderer and fetcher for each package. HTTPS feeds use certificate verification and a pinned public IPv4 connection for each request.

The `box_talk` source reads local firmware telemetry. Supported metrics are `room_humidity`, `room_temperature`, `electricity`, and `gas`. The `meter_archive` source displays recorded usage over the last 24 hours from the selected firmware meter archive. Electricity and gas need meter data. The solar tile can receive readings from an external `file` source, and the Energy screen uses the firmware's solar archive where present.

## Executable plugins

A package with `source: "wasm"` runs a WebAssembly module in a Wasmi interpreter. The package carries the module bytes as hexadecimal in the `wasm` field and can request a supported read-only telemetry metric with `metric`. The module receives that metric's formatted UTF-8 value at address zero. A module with no metric receives empty input.

Export linear memory as `memory` and a function `render(i32) -> i64`. The argument is the input byte length. Return the output address in the high 32 bits and the UTF-8 byte length in the low 32 bits. The output becomes the tile's text. Execution uses 100,000 fuel, at most 128 KiB of linear memory, and at most 256 output bytes. The host supplies no imports, filesystem, network, or heating controls. The data worker runs plugins outside the touchscreen loop. A trap produces an unavailable tile.

The Room summary package contains executable code, with its WAT source in the plugin repository. Weather and existing declarative feeds retain their existing package interface.
