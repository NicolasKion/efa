# efa

EVE Online fitting tool in the spirit of pyfa: a GPUI (gpui-kit) front end on the
[refine](https://github.com/DarkFenX/refine) fitting engine, fed with static data from
[deimos](https://github.com/NicolasKion/deimos) releases.

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

Licensed AGPL-3.0-or-later, since refine is.
