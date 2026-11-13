# ToonShift plugin repositories

The Store reads built-in widgets and local JSON files in `/mnt/data/toon-rust/catalog`. To add any number of sources within the catalog limits, sync each public GitHub repository independently. The same source filename is updated atomically, and other repositories keep their files.

```powershell
./scripts/sync-plugin-repo.ps1 -Toon toon -Repository ThuGie/ToonShift-plugins -Commit <full-commit-sha>
./scripts/sync-plugin-repo.ps1 -Toon toon -Repository another-owner/another-catalog -Commit <full-commit-sha>
```

From the touchscreen, **Store → Catalog sources → Refresh official** downloads the official index and verifies it against the public key shipped with ToonShift. **Add GitHub source** accepts `owner/repository` for its main branch or `owner/repository@<full-commit-sha>` for a pinned revision. Provision the community publisher's Ed25519 public key independently before adding the source. Save the 32-byte raw public key as 64 hex digits in `/mnt/data/toon-rust/catalog-keys/owner--repository.pub`, using lowercase owner and repository names. **Install** downloads only the selected package and checks its SHA-256, ID, and release. **Update** preserves tile placement and location settings.

The PowerShell script invokes the device's signed catalog sync over SSH. Refresh legacy GitHub sources once after updating ToonShift to replace the old cache with a signed copy. Existing package files and settings are reused after refresh.

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

Use a lowercase ID with digits and hyphens, up to 40 characters. This inline example is for locally managed catalogs. With `file`, a collector publishes one UTF-8 line to `/mnt/data/toon-rust/values/<id>.txt` using a temporary file and rename. Files must be at most 1,024 bytes.

Each version 1 catalog has room for 64 entries within 32 KiB. The app reads up to 64 catalog files and 128 total widgets. Built-in IDs take precedence, followed by catalog filenames in sorted order. Use a publisher prefix to avoid collisions.

For GitHub downloads, include `"artifact": {"path":"plugins/yourname-widget.json","sha256":"…"}` in every index entry. Use `"source":"file"` as the listing's placeholder. The package defines the real source, and shares the entry's `id` and `release`. The SHA-256 covers the exact UTF-8 bytes served by GitHub. Publish a detached Ed25519 signature as `catalog.sig`, containing 128 hex digits over the exact bytes of `catalog.json`. Publish both files in the same commit.

The generic `https_json` source accepts an HTTPS URL template, a numeric JSON pointer, suffix and precision, and optional detail labels, range rows, locations, and location-search schema. Weather's Open-Meteo endpoints, field mappings, and English and Dutch condition labels are in its own package. ToonShift runs the same renderer and fetcher for each package. HTTPS feeds use certificate verification and a pinned public IPv4 connection for each request.

The `box_talk` source reads local firmware telemetry. Supported metrics are `room_humidity`, `room_temperature`, `electricity`, and `gas`. The `meter_archive` source displays recorded usage over the last 24 hours from the selected firmware meter archive. Electricity and gas need meter data. The solar tile can receive readings from an external `file` source, and the Energy screen uses the firmware's solar archive where present.

## Executable plugins

A package with `source: "wasm"` runs a WebAssembly module in a Wasmi interpreter. The package carries the module bytes as hexadecimal in the `wasm` field and can request a supported read-only telemetry metric with `metric`. The module receives that metric's formatted UTF-8 value at address zero. A module with no metric receives empty input.

An `https_json` package can also include `wasm`. ToonShift fetches the configured public HTTPS URL and copies the response into the module's exported `input_buffer() -> i32`. Two bytes precede the response. The first is 0 for forecast or 1 for location search. The second is 0 for English or 1 for Dutch. The module's `render(i32) -> i64` receives the total input length and returns an output pointer in the high 32 bits and its length in the low 32 bits. The output is UTF-8 JSON up to 4 KiB, either `{value, detail, rows: [{date, range, condition}]}` or an array of `{name, latitude, longitude}` search results. Modules run without imports and with bounded memory and fuel. Existing declarative packages still load.

The Store checks WebAssembly imports, exported signatures and initial memory limits before activating an update. After a successful update, **Roll back** restores the previous cached package with the same plugin ID, dashboard position and settings.

Export linear memory as `memory` and a function `render(i32) -> i64`. The argument is the input byte length. Return the output address in the high 32 bits and the UTF-8 byte length in the low 32 bits. The output becomes the tile's text. Execution uses 100,000 fuel, at most 128 KiB of linear memory, and at most 256 output bytes. The host supplies no imports, filesystem, network, or heating controls. The data worker runs plugins outside the touchscreen loop. A trap produces an unavailable tile.

The Room summary package contains executable code, with its WAT source in the plugin repository. Weather and existing declarative feeds retain their existing package interface.
The Store's **Catalog sources** screen lists cached GitHub repositories and their revisions. Removing a source hides its widgets until it is added again. The downloaded packages and dashboard settings stay on the device. Community widget IDs include the lowercased GitHub owner and repository. Existing official IDs retain their original format. A unique matching legacy community widget is migrated with its tile positions and location settings. Ambiguous legacy IDs stay unresolved.
