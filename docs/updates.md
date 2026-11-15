# ToonShift software updates

GitHub Actions builds each push and pull request. To publish a stable update, merge the change into main, set the package version in Cargo.toml and Cargo.lock, then push the matching vX.Y.Z tag from main. The protected tag and stable-release environment gate publication. GitHub publishes the signed manifest and ARM binary as an immutable release.

The stable-release environment contains TOONSHIFT_RELEASE_SIGNING_KEY as an Ed25519 PEM. Keep a private offline backup. The public key is pinned in the updater. If the key is compromised, disable publication and automatic updates, replace the key and supervisor via the trusted bootstrap path, and restart stable publication at a higher version. The plugin catalog key has a separate trust boundary.

## Bootstrap and recovery

On a Toon 2 running firmware 6.4.5, download a known-good release through a trusted workstation, verify the release manifest signature and binary hash, then stage the binary and launcher script via authenticated SSH. Keep copies of the running executable and launcher.sh. Probe the candidate with --probe. Switch to stock with the installed switch-ui.sh, wait until the previous process exits, then install the candidate at toon-rust and copy that same binary to update-supervisor. Install the new launcher.sh and switch back to rust. Verify the selected inittab entry, UI process, and meter history. Further updates replace only toon-rust.

On the device use Settings → Software updates to check, install, disable automatic updates, or restore the previous version. Automatic installations require five minutes without touch and completed writes. The supervisor checks the framebuffer, touch input and thermostat response during startup, then watches the candidate for one minute. It restores the old executable if activation is interrupted or a candidate fails. If the restored UI fails, the launcher starts stock Qt. Rejected versions are bypassed until a new stable version is published.

The existing switch-ui.sh stock remains the recovery entry point. After returning to stock, use switch-ui.sh rust to re-enter ToonShift. If both update copies are unusable, stage the trusted bootstrap again while stock is running. Never remove household data files to recover the launcher.
