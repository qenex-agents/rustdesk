# QENEX seat binding

QENEX runs one autonomous bot per seat. Every seat has a private desktop (Xvfb +
fluxbox + Chrome + RFB) and a private working tree on the seat host. The RustDesk
client a bot drives has to keep all of its state inside that seat, otherwise one
bot can read or clobber the client state of another bot, or of the host operator.

## Environment

| Variable | Meaning |
|---|---|
| `QENEX_SEAT` | Seat identifier, e.g. `coder`. Absent or empty means the client is **unbound** and keeps the legacy, global behaviour. |
| `QENEX_SEAT_WORKSPACE` | Absolute path of the seat working tree. The only writable root while the client is seat-bound. |
| `QENEX_SEAT_ALLOW_GLOBAL` | Set to `1` to disable the confinement (operator/maintenance escape hatch). |

`QENEX_SEAT_TOKEN` and `QENEX_SEAT_API` are not used by the client and are
never read, logged or written.

## Layout

While the client is seat-bound every writable root lives in the seat workspace:

```
$QENEX_SEAT_WORKSPACE/
  .config/RustDesk/   # configuration
  .data/RustDesk/     # application data
  .cache/RustDesk/    # caches
  .state/RustDesk/    # other state
  .tmp/               # scratch space
  home/               # downloads, logs, autostart entries
```

`RustDesk` is the application name the client already uses, so these are exactly
the directories the client writes to.

## Behaviour

* **Unbound** (`QENEX_SEAT` unset or empty): nothing changes. The client keeps
  using the global configuration and home directories.
* **Bound** (`QENEX_SEAT` set and non-empty): the configuration, data, cache,
  state, temporary and home roots are redirected into `QENEX_SEAT_WORKSPACE`
  before any client state is touched, and the roots the client ends up using are
  verified to be inside the workspace. A write outside of the workspace is
  refused with an error that names the offending path:

  ```
  seat 'coder' refuses to write '/etc/rustdesk/config.toml': the path is outside the seat workspace '/var/lib/qenex/seats/coder/workspace'; set QENEX_SEAT_ALLOW_GLOBAL=1 to disable seat confinement
  ```

* **Seat identity** is visible in the version output:

  ```
  $ rustdesk --version
  1.2.3 seat=coder workspace=/var/lib/qenex/seats/coder/workspace
  ```

  An unbound client prints the version only, it never claims a seat.

## Escape hatch

`QENEX_SEAT_ALLOW_GLOBAL=1` keeps the legacy global roots. The seat identity is
still reported, so it stays visible that the confinement is disabled.

## Notes

* The configuration and home directories are resolved by `hbb_common`, which is a
  git submodule of this repository and therefore not changed by this patch. The
  redirection is applied to the process environment before the first client state
  is touched, and the resolved roots are verified afterwards, so a platform that
  cannot redirect them fails with the refusal above instead of writing outside of
  the workspace.
* Reads are never restricted: the client can still read system and toolchain
  locations.

## Tests

`libs/base/src/seat.rs` holds the seat logic and its unit tests, one per state:
unbound keeps the legacy behaviour, bound resolves the roots inside the seat
workspace, and bound plus a write outside the workspace is refused.

```
cargo test -p base seat
```
