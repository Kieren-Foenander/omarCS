# Vendored demoparser core

`src/parser` and `src/csgoproto` originate from
[`LaihoE/demoparser` commit `57f24c76776ac176e893833f3a5b4aad718a8196`](https://github.com/LaihoE/demoparser/tree/57f24c76776ac176e893833f3a5b4aad718a8196).
The upstream code is MIT licensed; see `LICENSE`. The upstream-only
`src/parser/test_demo.dem` fixture is not distributed with omarCS.

`OMARCS.patch.gz` contains the complete, machine-applicable difference between
that upstream revision and the two crates distributed here. It includes every
behavioral, generated, formatting, manifest, test, and build-script change;
the prose summary below is not a substitute for the compressed patch.

## Changes from upstream

- Build scripts use the checked-in protobuf and item-map outputs. They do not
  clone or execute remote source during an omarCS build.
- The generated protobuf bindings are updated and checked in. Related parser
  tables, fixtures, and formatting changes needed by those bindings are in the
  patch.
- A compact, column-oriented Match Facts tick accumulator is populated during
  the parser's full-packet second pass. This avoids constructing the generic
  dataframe for omarCS while retaining the upstream parsing design.
- Player-referencing game events include the compact team number from parser
  metadata, which Match Facts use for round-side and enemy/friendly attribution.
- Crate-local profile and build dependencies are removed where the containing
  omarCS workspace supplies or no longer needs them.
- The maintainer-only `update_protos.py` helper requires a local
  `GameTracking-CS2` checkout and never downloads it. The directory is ignored
  and is not part of the distributed plugin.

## Reproduce and verify

The GitHub source archive and the patch used for this release have these
SHA-256 digests:

```text
a3185b123408deb44304b2c7d2927e4b38ba6aac70b49727d7a75ec6dac769ef  demoparser-upstream.tar.gz
2726b5424a92b5880608d4fd7c62b8f14877d38e35215c1cfd0648534a9a862f  OMARCS.patch.gz
```

A reviewer can download the source archive for the pinned commit, verify the
first digest, extract it, and run this from the extracted upstream root:

```sh
gzip -dc /path/to/omarCS/vendor/demoparser/OMARCS.patch.gz | git apply
diff -ru --exclude=test_demo.dem src/parser /path/to/omarCS/vendor/demoparser/src/parser
diff -ru --exclude=GameTracking-CS2 src/csgoproto /path/to/omarCS/vendor/demoparser/src/csgoproto
```

Both `diff` commands produce no output. `GameTracking-CS2` is excluded only so
a maintainer's optional local checkout cannot affect the comparison; it is not
tracked by omarCS.
