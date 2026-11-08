# efa

EVE Online fitting tool in the spirit of pyfa: a GPUI (gpui-kit) front end on the
[refine](https://github.com/DarkFenX/refine) fitting engine, fed with static data from
[deimos](https://github.com/NicolasKion/deimos) releases.

## Install

Downloads are on the [releases page](https://github.com/NicolasKion/efa/releases). The builds are
not signed with a paid certificate, so macOS and Windows warn on first start.

- macOS: open the `.dmg` for your Mac (`aarch64` for Apple silicon, `x64` for Intel) and drag efa
  into Applications. If macOS says the app is damaged or cannot be checked, run
  `xattr -dr com.apple.quarantine /Applications/efa.app` once, or right-click it and pick Open.
- Windows: run `efa_<version>_x64-setup.exe`, which installs for your user without admin rights.
  SmartScreen may say it protected your PC: click More info, then Run anyway. The
  `x64-portable.exe` runs without installing but does not update itself.
- Linux: the AppImage (`chmod +x`, then run it; it adds itself to the app launcher), the `.deb`, or
  `efa-bin` from the AUR. The tarball has the plain binary, a desktop entry and icons.

The AppImage, the macOS app and the Windows installer update themselves: efa checks at startup and
every hour (Preferences > Network > Updates), shows a notice when a release is out and restarts
into it. The other builds link to the release page. Preview builds (versions like
`0.3.0-preview.1`) also hear about newer previews.

## Development

```sh
scripts/fetch-data.sh        # latest deimos dump into data/eve
cargo run --release          # or: cargo run --release -- some-fit.eft
```

refine needs nightly Rust; `rust-toolchain.toml` pins a version that builds it.

## EVE data

Releases carry their EVE data inside the binary, as a data pack: the catalog, the item stats data
and refine's adapted data, built ahead of time from one deimos release and stored as zstd-compressed
postcard. Starting from it reads no JSON and downloads no EVE data; only icons, prices and app
updates come from the network.

```sh
scripts/build-pack.sh        # fetch-data.sh, then efa --build-pack data/eve assets/data/efa-data.pack
cargo build --release        # embeds assets/data/efa-data.pack
```

`build.rs` embeds `assets/data/efa-data.pack`, or the file `EFA_PACK` names (the build fails if
`EFA_PACK` is set and the file is missing). The pack is never committed. The same dump gives the
same pack, byte for byte. A pack records its format, the catalog and item data formats and the
refine commit its engine data was made with; a build that differs in any of them refuses it with a
message to rebuild it, so build the pack with the same checkout that embeds it.

Without a pack, or with `EFA_DATA` set, efa reads a deimos dump (`EFA_DATA`, or `data/eve`) the way
development always has: the first start after a data update converts the data and caches it.

## Folders

User data (fits, characters, ESI logins, settings) and caches (icons, prices, CDN index, caches of
a dump) live in the platform's folders:

- Linux: `$XDG_DATA_HOME/efa` (`~/.local/share/efa`) and `$XDG_CACHE_HOME/efa` (`~/.cache/efa`)
- macOS: `~/Library/Application Support/efa` and `~/Library/Caches/efa`
- Windows: `%APPDATA%\efa` and `%LOCALAPPDATA%\efa`

Fits, characters, ESI logins and settings are kept in one SQLite database, `efa.db` in the data
folder, readable by the user only. Each start leaves a copy of it as `efa.db.bak`; a database that
cannot be read is renamed out of the way and the copy loads instead. The JSON files earlier versions
kept (`fits.json`, `characters.json`, `esi.json`) are imported on the first start and renamed to
`<name>.migrated`.

A gyfa folder from before the rename is copied over on the first start.

Item icons come from CCP's client CDN the first time they are shown. On Omarchy the app follows the
current theme (`~/.local/state/omarchy/current/theme/colors.toml`) and restyles itself when it
changes.

FEATURES.md and CONTEXT_MENUS.md track parity with pyfa, whose source is the reference for behaviour
and layout.

`cargo run --example stats < fit.eft` prints the engine's numbers for a fit without the UI.

`scripts/icons.sh` renders `assets/icons/efa-e.svg` into the PNG, `.ico` and `.icns` files the
packages use; run it after changing the SVG and commit the output.

## Releases

1. Bump `version` in `Cargo.toml` (a hyphen, as in `0.3.0-preview.1`, makes a pre-release).
2. Push to the `release` branch, e.g. `git push origin main:release`. The Release workflow writes
   the notes from the conventional commits since the last tag (`cliff.toml`), builds the data pack
   and every package, and uploads them to a draft release `v<version>`.
3. Check the draft and publish it. Installed copies pick it up from then on, and the AUR workflow
   pushes `efa-bin` for stable releases.

Repository settings the workflows read:

- Variable `UPDATER_PUBKEY` and secrets `UPDATER_PRIVATE_KEY`, `UPDATER_PRIVATE_KEY_PASSWORD`: the
  minisign key pair the updater checks downloads against. Make one with
  `cargo install cargo-packager --version 0.11.8 --locked` and
  `cargo packager signer generate --path efa.key`, then put the content of `efa.key.pub` in the
  variable, the content of `efa.key` in the first secret and its password in the second. Keep the key safe: builds that shipped with its public half only
  accept updates signed by it. Without these, releases build unsigned, no `latest.json` is
  published and the app only links to new releases.
- Secret `AUR_SSH_PRIVATE_KEY` (optional): an SSH key registered on the AUR account that owns
  `efa-bin`. Variables `AUR_COMMIT_NAME` and `AUR_COMMIT_EMAIL` set the commit author there.

Licensed AGPL-3.0-or-later, since refine is.
