# ToonShift plugin repositories

The Store reads built-in widgets and local JSON files in `/mnt/data/toon-rust/catalog`. To add any number of sources within the catalog limits, sync each public GitHub repository independently. The same source filename is updated atomically, and other repositories keep their files.

```powershell
./scripts/sync-plugin-repo.ps1 -Toon toon -Repository ThuGie/ToonShift-plugins -Commit <full-commit-sha>
./scripts/sync-plugin-repo.ps1 -Toon toon -Repository another-owner/another-catalog -Commit <full-commit-sha>
```

The script downloads `catalog.json` over HTTPS on your computer, validates it, and transfers it over SSH. Open Store again to refresh the list. To stop using a source, remove its `owner--repository.json` over SSH. Remove any installed tiles in Store before removing the source if you want them removed from your dashboard too. A missing source displays its installed widgets as **Unavailable**.

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

The official starter pack contains electricity, gas, solar, humidity, and weather display widgets. A compatible collector must provide their data. Native data collectors and direct Store repository management are tracked in the [roadmap](roadmap.md).
