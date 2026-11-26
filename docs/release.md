# Releasing a version

For the maintainer. A pushed tag `v*` starts the Release workflow (`.github/workflows/release.yml`), which publishes what `install.sh` downloads (see [install.md](install.md)).

1. Check that CI on `main` passes.
2. Set `version` in `Cargo.toml`, e.g. `0.2.0`, run `cargo check` so `Cargo.lock` follows, and commit both.
3. Tag the commit and push it:

   ```sh
   git tag v0.2.0
   git push origin main v0.2.0
   ```

The workflow then:

1. runs CI again on the tagged commit,
2. stops when the tag is not `v` plus the version in `Cargo.toml`,
3. builds `tsmusic-x86_64-unknown-linux-gnu` and `tsmusic-aarch64-unknown-linux-gnu` on Ubuntu 22.04 (glibc 2.35, so the bot runs on Debian 12, Ubuntu 22.04 and newer), with opus built in,
4. publishes the release with both binaries, `SHA256SUMS` and notes generated from the changes since the previous tag.

A tag with a suffix, e.g. `v0.2.0-rc.1`, gives a pre-release. `install.sh` skips it unless asked: `TSMUSIC_VERSION=v0.2.0-rc.1`.

## When it fails

Nothing is published before both builds pass. Delete the tag, fix the cause and tag again:

```sh
git push --delete origin v0.2.0
git tag -d v0.2.0
```

A published release is not replaced, a second run for the same tag fails. Delete the release on GitHub first, or release the next version.

## Requirements

- The repository is public: `install.sh` downloads the script, the binaries and the sums without logging in.
- The runners `ubuntu-22.04` and `ubuntu-22.04-arm` exist. When GitHub retires Ubuntu 22.04, the build moves to the next runner, and the oldest supported system moves with it.
