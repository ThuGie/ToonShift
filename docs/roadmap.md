# ToonShift roadmap

The target is a fast, local-first replacement for the entire Toon 2 touchscreen experience. Use firmware 6.4.5 on Toon 2 as the reference device. Record each stock screen, menu, setting, and hardware prerequisite. Keep a parity matrix with the stock behavior, ToonShift screen, backend contract, and verification result. Track settings that depend on a tenant, region, subscription, accessory, or discontinued service individually.

| Area | Implemented | Work to reach parity |
| --- | --- | --- |
| Home | Room temperature, target, schedule toggle, widget pages, persistent tile order | Direct drag with page-edge scrolling, multiple tile sizes, named pages, weather and usage summaries, configurable shortcuts and screen saver |
| Appearance | Three themes and automatic day/night colors | Per-mode brightness and contrast, idle dim and wake, configurable hours, ambient light option, touch calibration, localization and accessibility |
| Thermostat | Setpoint, program toggle, weekly schedule viewer | Comfort/Home/Sleep/Away presets, temporary hold, vacation, weekly editor and copy day, heating history, calibration, frost behavior, sensor and adapter faults |
| Boiler and HVAC | Basic burner widget | OpenTherm and on/off setup, adapter status, diagnostics, modulation, DHW, ventilation, heat recovery, zones and TRVs where present |
| Electricity and gas | Store tiles with a local value contract | Meter discovery and wizard, P1 and wireless meter adapters, tariffs, gas and electricity graphs, consumption/export, solar, unit conversion, missing readings |
| Connectivity | Wi-Fi scan and stock setup handoff | Join and forget networks, password entry, hidden SSIDs, DHCP/static IP, signal and diagnostics, time sync, recovery after a failed switch |
| Accessories | Firmware API inventory | Z-Wave inclusion/exclusion, node health, smart plugs, Hue, smoke detectors, configuration and recovery |
| Device | Build, SSH install, stock recovery | Setup wizard, locale, clock and timezone, offline/update status, settings export/import, logs, factory-reset workflow and signed update/rollback |
| Services | Local-first UI | Identify weather, messages, service and cloud features that need supported equivalents. Document per-region availability and privacy choices |
| Remote control | Bearer API over SSH | Android client, pairing, per-client permissions, token revocation, encrypted local HTTPS transport, read-only integrations and event streaming |

## 1. Make the current UI measurable

Add hardware benchmarks for input-to-frame latency, cold launch, repaint time, CPU load, and resident memory. Record median and p95 timings from the same Toon 2 in both UIs. Start with a p95 target under 80 ms for local navigation, a cold launch target under two seconds after the UI process starts, and under 2% idle CPU excluding existing firmware. Measure before claiming a speedup.

Keep I/O away from the input and paint loop. Cache immutable glyphs and icons, compare state before repainting, and invalidate only affected regions once full-frame timings show the benefit. Cap caches, queues, and decoded payloads. Run a 24-hour idle and active soak. Test repeated tapping, offline services, touch resynchronization, and framebuffer page offsets.

## 2. Make heating and device settings feature-complete

1. Capture read-only firmware fixtures and stock UI behavior. Validate unknown calls with a separate test setup before writing through them.
2. Implement presets, overrides, vacation, and the schedule editor against the existing scheduler. Validate time overlaps, weekday boundaries, time zones and DST. Require a consistent write/read-back and preserve existing programs on migration.
3. Add heating history, fault explanations, and adapter details. Verify on/off and OpenTherm behavior with the applicable hardware before supporting either in release notes.
4. Implement Wi-Fi credentials and network management using the firmware configuration service once its write contract is confirmed. Exercise failed passwords, lost DHCP, and network recovery using physical access. Preserve an on-device path to repair connectivity.
5. Complete stock device preferences, installer screens, and peripheral commissioning. Verify each hardware-dependent screen using fixtures and at least one matching real accessory.

## 3. Extend the dashboard and Store

Use the current persistent tile ordering as the base. Add drag previews, edge paging, resize handles, page names, and a resettable layout. Keep a tap-based reordering path. Add widget-specific settings and consistent loading, unavailable, and stale-data states.

Add an in-app source manager for the [official repository](https://github.com/ThuGie/ToonShift-plugins) and community sources. Show publisher and version, fetch catalogs with a maintained TLS implementation, and verify signed version metadata. Stage updates and keep the previous catalog if download or verification fails. Namespace IDs by publisher and migrate installed IDs. Include source removal, version pinning, and update history. Keep catalog parsing and widget configuration declarative. If a plugin eventually needs computation, define a narrowly scoped capability interface with resource limits.

Prioritize independent integrations for common ToonStore use cases. Start with thermostat controls, P1 power and gas, solar, weather/radar, Home Assistant, and air-quality data. Follow with smart plugs, lighting, calendars, and notifications. Verify data sources, respect upstream licenses, and test device resource use for each addition.

## 4. Build Android and integration support

Define a versioned schema for thermostat status, schedules, device health, and approved control operations. Add an event stream so clients can subscribe instead of polling. Start the Android client with the existing SSH tunnel. For direct LAN use, add device identity and pairing with a short-lived code, encrypted transport, per-client credentials, scoped grants, and revocation. Keep legacy firmware actions outside the app's public API. Verify failed authentication, token rotation, concurrent writes, response timeouts, and reconnection.

## 5. Release and migration

Add a host-side simulator and ARM build checks, snapshot key screens, and automate firmware response fixtures. Validate upgrade, downgrade, power loss during updates, and stock recovery on real hardware. Publish signed release artifacts with an explicit compatibility matrix and data-migration notes. Ship each parity area when its hardware and failure tests pass.
