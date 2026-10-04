# Roadmap

- [x] Set the default Minecraft memory allocation to 6 GB.
- [x] Preserve player data, mod configuration, and shader data across updates; provide a setting to include or exclude this persistent data during updates.
- [x] Add the Mars server to the Minecraft server list.
- [x] Add quick actions to report a bug, open the installation folder, and visit the GitHub repository and project website; include accessibility settings.
- [x] Close Mars Command when Minecraft itself starts, scoped to the configured Mars game directory with a two-minute timeout.
- [x] Add a Fund This Project button linking to GitHub Sponsors. Future supporter benefits remain prospective.
- [x] Check GitHub releases and parse installer versions from `setup-X.Y.Z.exe` and `setup-X.Y.Z.deb` assets.
- [x] Add a tag-triggered Windows/Linux release workflow that publishes versioned installer assets.
- [ ] Push a `client-vX.Y.Z` tag to publish the Windows and Linux installers.
- [x] Add Mission Control configuration for the dashboard status probe.
- [ ] Add Crew roster and messaging after a server API contract is available.
- [x] Implement Linux Minecraft profile setup and launcher opening through `~/.minecraft` and `minecraft-launcher`; Linux build and Rust tests pass in Docker.
- [x] Build and install the generated Linux `.deb` in a Linux container; verify its package metadata and installed launcher files.
