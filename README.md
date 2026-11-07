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

refine needs nightly Rust; `rust-toolchain.toml` pins a version that builds it. The first start
after a data update converts the data for refine and caches it in `data/cache`. `EFA_DATA` points
at a different dump.

Item icons come from CCP's client CDN the first time they are shown and are cached in
`data/cache/icons`; attribute and market group icons ship with the deimos dump. On Omarchy the app
follows the current theme (`~/.local/state/omarchy/current/theme/colors.toml`) and restyles itself
when it changes.

The character has every skill at V. Fits are kept in `~/.local/share/efa/fits.json`, and EFT goes both ways (import
dialog, copy to clipboard).

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
