# ToonShift plugin repositories

## Install Tuya and Home Assistant

Refresh the official catalog, install **Tuya / Smart Life** or update **Home Assistant**, and tap **Open** in the Store or use the dashboard tile. Both require a launcher that supports interactive WebAssembly plugins.

- **Tuya / Smart Life** connects directly to Tuya Cloud. First link the Smart Life account in a Tuya Cloud project with device read and control permissions. On the Toon, enter that project's region, Access ID, Access Secret, and the linked Smart Life UID. Approve the HTTPS destinations and tap **Connect and save**. The region codes are `eu` for Central Europe, `we` for Western Europe, `us` for Western America, `ue` for Eastern America, `cn` for China, and `in` for India. The plugin discovers online devices with boolean `switch` or numbered `switch_N` channels and reads back each command. [Tuya device list contract](https://developer.tuya.com/en/docs/cloud/ad2823ae46?id=Kconjtzq1vk1q)
- **Home Assistant** takes an HTTPS origin such as `https://ha.example.net:8123` and a long-lived access token. Approve that exact endpoint and tap **Connect and save**. It discovers switches and confirms changes via the REST API. This release uses its own settings. Re-enter the connection if you used Smart Home Controls 1.1. [Home Assistant REST API](https://contributors.home-assistant.io/core/internals/rest-api/)

Connection credentials are stored locally with owner-only file permissions. **Remove connection** clears the plugin's saved settings. Revoke the old credential at its provider when retiring a connection. For Home Assistant HTTPS using a private CA, see the per-plugin CA path below.

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

A package with `source: "wasm"` runs a WebAssembly module in Wasmi. Put the compiled binary under `modules/` and declare it as `"module": {"path":"modules/example-1.0.0.wasm","sha256":"…"}`. The signed catalog authenticates the package hash, and the package authenticates the WebAssembly hash. ToonShift verifies the binary before activation and keeps it available by hash for rollback. Previously installed hex-encoded modules still load. A module can request a supported read-only telemetry metric with `metric` and receives its formatted UTF-8 value at address zero.

An `https_json` package can also include `wasm`. ToonShift fetches the configured public HTTPS URL and copies the response into the module's exported `input_buffer() -> i32`. Two bytes precede the response. The first is 0 for forecast or 1 for location search. The second is 0 for English or 1 for Dutch. The module's `render(i32) -> i64` receives the total input length and returns an output pointer in the high 32 bits and its length in the low 32 bits. The output is UTF-8 JSON up to 4 KiB, either `{value, detail, rows: [{date, range, condition}]}` or an array of `{name, latitude, longitude}` search results. Modules run without imports and with bounded memory and fuel. Existing declarative packages still load.

The Store checks WebAssembly imports, exported signatures and initial memory limits before activating an update. After a successful update, **Roll back** restores the previous cached package with the same plugin ID, dashboard position and settings.

Export linear memory as `memory` and a function `render(i32) -> i64`. The argument is the input byte length. Return the output address in the high 32 bits and the UTF-8 byte length in the low 32 bits. The output becomes the tile's text. Execution uses 100,000 fuel, at most 128 KiB of linear memory, and at most 256 output bytes. The host supplies no imports, filesystem, network, or heating controls. The data worker runs plugins outside the touchscreen loop. A trap produces an unavailable tile.

The Room summary package contains executable code, with its WAT source in the plugin repository. Weather and existing declarative feeds retain their existing package interface.
The Store's **Catalog sources** screen lists cached GitHub repositories and their revisions. Removing a source hides its widgets until it is added again. The downloaded packages and dashboard settings stay on the device. Community widget IDs include the lowercased GitHub owner and repository. Existing official IDs retain their original format. A unique matching legacy community widget is migrated with its tile positions and location settings. Ambiguous legacy IDs stay unresolved.


## Interactive screens ABI version 1

Publish a package with `"source": "interactive"` and a standalone module under `modules/`. Use the signed package and catalog format above. The module exports `memory`, `input_buffer() -> i32` and `render(i32) -> i64`. The host calls it with a single JSON input and receives one JSON output. Package updates supply new screens and actions through the same ABI.

An input includes `version: 1`, `action` (initially `open`, then the tapped button ID), `language` (`en` or `nl`), `fields` (an object keyed by field ID), `state` (initially saved state or null), and `timestamp_ms` (Unix milliseconds). Each call starts a fresh module instance. Use `state` to carry session data.

A screen output can look like this.

```json
{
  "screen": {
    "message": "Enter a display name.",
    "fields": [{"id":"name", "label":"Name", "value":"", "secret":false}],
    "buttons": [{"id":"apply", "label":"Apply"}]
  },
  "state": null
}
```

ToonShift draws the plugin title, message, input controls, masked secrets, and buttons in its native interface. It provides the keyboard and pagination. Fields can be marked `readonly`. A screen can have up to 12 fields and 48 buttons. IDs may contain ASCII letters, digits, underscores, and hyphens, and must be unique within their list. Use short labels that fit a two-column touchscreen grid. Modules have 512 KiB of linear memory, 4 million fuel per call, up to 32 KiB input and up to 4 KiB output. Compiled binaries must fit within 128 KiB.

Set `save: true` on a successful screen response to atomically replace the persistent state. This is available after a user action. Use `state: null` with `save: true` for disconnect. Each file is scoped to the full catalog ID, including a community publisher namespace. The filename is `extension-<sha256-of-full-id>.json`. A custom public CA certificate for a user-approved origin can be installed at `extension-<sha256-of-full-id>-ca.pem` in the data directory. Private keys belong on the server.

A plugin can declare either `https_hosts` (up to eight exact public DNS names) or `user_https_origin: true`. For the latter, render a field with ID `https_origin` containing an HTTPS origin without a path. Its current value is shown for user approval. Use `"host":"user"` in requests for that origin. For public HTTPS use a declared hostname in `host`. The host checks the user's grants, verifies TLS and pins the resolved IPv4 address. It omits redirects and proxies, excludes loopback and link-local addresses, and only permits private IPv4 for the individually approved origin. A new session asks for approval again.

After a button tap a module can return `{"request":{"host":"api.example.net","path":"/v1/status","headers":{}},"state":{"phase":"refresh"}}`. A JSON object or array encoded as a string in `body` selects POST. Omitting `body` selects GET. The host feeds the JSON result back with the same action, a one-based `step`, and `response: {"data": ...}` or `response: {"error": "..."}`. Finish the chain with a screen. The host allows at most six requests per tap. Opening a screen permits no requests. For a top-level array response, `request.array` can supply `prefix_pointer`, `prefix`, `pointers` (one to eight JSON pointers), `offset`, and `limit`. Each selected row becomes an array of values in pointer order.

Keep provider state and protocol code in the WebAssembly module. The module has no host imports and shares no filesystem, firmware control or shell access. A publisher can keep credentials in its own private state and use them with the destinations the user approved. Install plugins from publishers you trust with the credentials you enter. [Tuya and Home Assistant source examples](https://github.com/ThuGie/ToonShift-plugins/tree/main/source)

### Read a local meter

An interactive package can declare a single `lan_read_path` for a legacy LAN measurement API. It presents a field with ID `lan_address`. ToonShift asks the user to approve the private IPv4 address and exact path before the plugin can read it. The module requests `{"host":"user","path":"/api/v1/data"}`. The host only permits a GET to that path on port 80, with no headers, body, credentials, query, or redirects. This uses unencrypted local HTTP. The LAN permission cannot be combined with public or user-configured HTTPS permissions.
