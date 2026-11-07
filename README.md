# efa

EVE Online fitting tool in the spirit of pyfa: a GPUI (gpui-kit) front end on the
[refine](https://github.com/DarkFenX/refine) fitting engine, fed with static data from
[deimos](https://github.com/NicolasKion/deimos) releases.

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
same pack, byte for byte, and a pack only fits the build of efa and refine that made it: an older
one is refused with a message to rebuild it.

Without a pack, or with `EFA_DATA` set, efa reads a deimos dump (`EFA_DATA`, or `data/eve`) the way
development always has: the first start after a data update converts the data and caches it.

## Folders

User data (fits, characters, ESI logins, settings) and caches (icons, prices, CDN index, caches of
a dump) live in the platform's folders:

- Linux: `$XDG_DATA_HOME/efa` (`~/.local/share/efa`) and `$XDG_CACHE_HOME/efa` (`~/.cache/efa`)
- macOS: `~/Library/Application Support/efa` and `~/Library/Caches/efa`
- Windows: `%APPDATA%\efa` and `%LOCALAPPDATA%\efa`

A gyfa folder from before the rename is copied over on the first start.

Item icons come from CCP's client CDN the first time they are shown. On Omarchy the app follows the
current theme (`~/.local/state/omarchy/current/theme/colors.toml`) and restyles itself when it
changes.

FEATURES.md and CONTEXT_MENUS.md track parity with pyfa, whose source is the reference for behaviour
and layout.

`cargo run --example stats < fit.eft` prints the engine's numbers for a fit without the UI.

Licensed AGPL-3.0-or-later, since refine is.
