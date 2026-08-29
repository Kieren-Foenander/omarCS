# Vendored demoparser core

This directory contains the Rust parser and protobuf crates from [`LaihoE/demoparser`](https://github.com/LaihoE/demoparser) at revision `57f24c76776ac176e893833f3a5b4aad718a8196`.

The upstream code is MIT licensed; see `LICENSE`. omarCS vendors it so parser output construction can be replaced with compact Match Facts accumulation while retaining its full-packet Rayon parsing design.

omarCS enriches player-referencing game events with the player's compact team
number from parser metadata. Upstream's generic requested-property path does
not expose this field consistently, while Match Facts require it for exact
round-side and enemy/friendly attribution.
