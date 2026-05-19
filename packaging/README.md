# Packaging

gamerat ships as four distro-format artifacts on every tagged release.
All four contain the exact same payload at the exact same install paths;
the only differences are which package manager owns the metadata and
which distros they're tested on.

| Artifact                             | Format         | Tested on                                  |
|--------------------------------------|----------------|---------------------------------------------|
| `gamerat-${ver}-1-x86_64.pkg.tar.zst`| Arch package   | Arch Linux                                  |
| `gamerat_${ver}-1_amd64.deb`         | Debian package | Debian 12 (bookworm), Ubuntu 24.04 (noble) |
| `gamerat-${ver}-1.x86_64.rpm`        | RPM            | Fedora (latest)                             |
| `gamerat-${ver}-x86_64-linux.tar.gz` | Binary tarball | glibc 2.36+ baseline (Debian 12 / Fedora 38 / Ubuntu 24.04 / Arch) |

The releases page is at <https://github.com/appulsauce/gamerat/releases>.

## Install

```sh
# Arch / Manjaro / EndeavourOS
sudo pacman -U gamerat-*.pkg.tar.zst

# Debian 12+ / Ubuntu 24.04+
sudo apt install ./gamerat_*.deb

# Fedora 38+ / openSUSE Tumbleweed (RPM is portable to other DNF-using distros)
sudo dnf install ./gamerat-*.rpm

# Anywhere with glibc 2.36+ and webkit2gtk-4.1 on the system
tar xzf gamerat-*-x86_64-linux.tar.gz
cd gamerat-*
sudo ./install.sh                    # PREFIX=/usr/local by default
# or
PREFIX="${HOME}/.local" ./install.sh # per-user install (no sudo needed)
```

After install:

```sh
sudo systemctl enable --now ratbagd        # the upstream daemon gamerat sits on top of
systemctl --user enable --now gamerat-daemon
```

Then launch `gamerat-gui` from your application menu, or run it directly.

## ABI / version floor

The GUI links against `webkit2gtk-4.1`. That sets the minimum supported
versions:

- Debian 12 (bookworm) or newer
- Ubuntu 24.04 LTS (noble) or newer — **Ubuntu 22.04 jammy still ships
  webkit2gtk-4.0 and is not supported**
- Fedora 38 or newer
- Arch / rolling

The tarball additionally needs glibc 2.36+ (the build container is Debian
12, which is the floor; older RHEL 8 / Debian 11 / Ubuntu 22.04 are out).

## Per-distro smoke testing

Each tagged release runs `scripts/install-smoke.sh` inside a fresh
container of every target distro before publishing. The smoke covers:

- All three binaries present + responding to `--version`
- systemd unit syntactically valid (`systemd-analyze --user verify`)
- D-Bus interface XML well-formed (`xmllint --noout`)
- `.desktop` entry valid (`desktop-file-validate`)
- Icon file present at `/usr/share/icons/hicolor/512x512/apps/gamerat.png`
- Mouse SVG + KWin script bundles present at their expected paths
- Daemon claims `org.appulsauce.GameRat1` on a fresh session bus within 3 s
  (using `gamerat-daemon --no-ratbagd` so it runs without ratbagd in the
  container)

There's no peripheral testing — hardware behaviour is out of scope for
package CI. Manual verification on a real device is the responsibility of
the maintainer before a stable release.

## Local CI iteration via `act`

[`nektos/act`](https://github.com/nektos/act) runs the workflow locally
inside docker. The release.yml file is `act`-clean except for the
`gh-release` and `aur-publish` jobs, which need real GitHub Releases /
AUR endpoints and are skipped locally.

```sh
# Single job:
bash scripts/act-smoke.sh build-deb       # build the .deb in debian:bookworm
bash scripts/act-smoke.sh smoke-deb       # download + install + smoke in matrix

# Cycle through every distro:
for j in build-arch smoke-arch build-deb smoke-deb build-rpm smoke-rpm build-tarball; do
    bash scripts/act-smoke.sh "$j"
done
```

`scripts/act-smoke.sh` does disciplined cleanup after each run: every
container and image act spawned during the run is torn down on exit
(success or fail), with the single exception of the `catthehacker/ubuntu`
base — that one's ~2 GB and re-pulling it every iteration would dominate
wall-clock time. The distro containers (`archlinux`, `debian:bookworm`,
`fedora:latest`, `ubuntu:24.04`) are full-tear-down each run, sacrificing
re-pull time for bounded disk usage.

## Versioning

Each tagged release flows through `scripts/stamp-version.sh` once per
build job (after `actions/checkout@v4`), rewriting the workspace's root
`Cargo.toml` version and the `PKGBUILD` `pkgver` in lockstep. The
lockfile is refreshed offline in the same step so downstream `cargo
build` calls succeed without `--locked` drift complaints.

## Troubleshooting

- **`gamerat-gui` exits with `error while loading shared libraries:
  libwebkit2gtk-4.1.so.0`** — your distro ships webkit2gtk-4.0 (Ubuntu
  22.04 and older). Upgrade or build from source against the 4.0 line
  (not currently a supported configuration).

- **`gamerat-daemon` exits with "connecting to ratbagd"** — ratbagd
  isn't running or you don't have permission to talk to it on the system
  bus. Start it with `sudo systemctl enable --now ratbagd` and verify
  with `systemctl status ratbagd`. As a temporary workaround you can run
  `gamerat-daemon --no-ratbagd`; the GUI / CLI will surface a clear
  "ratbag integration disabled" error for any mouse operation.

- **The launcher tile doesn't show up after install** — `gtk-update-icon-cache`
  or `update-desktop-database` failed silently. Re-run the install hook
  by force: `sudo gtk-update-icon-cache --force /usr/share/icons/hicolor &&
  sudo update-desktop-database /usr/share/applications`.

- **AUR push not happening on tag releases** — the workflow gates AUR
  publish on `secrets.AUR_SSH_PRIVATE_KEY`. Until that secret is set in
  the repo's GitHub settings, the `aur-publish` job runs a no-op step
  that logs `"AUR_SSH_PRIVATE_KEY not configured; skipping AUR push."`
  and exits 0. Once configured, every subsequent tag auto-pushes.
