# systemd unit

`gamerat.service` is a user-scoped service that runs the daemon for the
duration of the graphical session. Install with:

```sh
mkdir -p ~/.config/systemd/user
cp gamerat.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now gamerat.service
```

Hardening defaults (`ProtectSystem=strict`, `PrivateNetwork=true`, …)
are intentional and may need loosening once the daemon grows real
features. Treat this file as a *starting point*, not a final spec.
