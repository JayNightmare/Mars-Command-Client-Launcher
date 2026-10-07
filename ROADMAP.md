# Roadmap

## Milestone 1: Personal client mods

- [x] Add a mod selection/upload flow for a user's isolated Mars installation.
- [x] Validate supported file type, size, duplicate/conflicting files, and Minecraft/NeoForge compatibility before install; show clear errors and compatibility warnings.
- [x] Install and remove user mods in the isolated instance without changing the signed base pack or shared `.minecraft/mods` directory.
- [x] Preserve user-installed mods during normal signed pack updates, and make their status visible in integrity results.

## Milestone 2: Remote mod library

- [ ] Add authenticated mod submission to remote storage with metadata for author, version, game/loader compatibility, source, and license.
- [ ] Validate submissions before publishing; retain checksums and version history so downloads can be verified.
- [ ] Publish validated submissions automatically and provide report and takedown controls.
- [ ] Keep community files separate from the signed base-pack manifest and provide clear upload limits and failure messages.

## Milestone 3: Community tab

- [ ] Add a community tab to browse and search published client mods, with compatibility and provenance details.
- [ ] Let users install and remove community mods into their own isolated Mars installation.
- [ ] Verify downloaded files against their published checksums and warn or block when compatibility or integrity checks fail.
- [ ] Show installed mod versions and make updates/removals explicit and reversible.

## Milestone 4: Donation-based server role

- [ ] Define how donation status is verified and synchronized with the server.
- [ ] Grant and revoke the Premium Player role from verified donation status; keep authorization server-side.
- [ ] Handle delayed, refunded, or revoked donations without granting roles from client-reported status.

## Milestone 5: Client update completion

- [x] Complete the Update Client flow around the existing stable-release check: clearly present the available installer, require the user's confirmation, and explain how to finish installation.
- [x] Document supported release assets and recovery behavior when a release or download is unavailable.

## Existing foundations

The launcher already verifies and syncs the signed base pack into an isolated game directory, checks stable GitHub releases for client installers, and opens the selected installer for the user. Community mods must remain user-controlled and separate from that trusted base pack.
