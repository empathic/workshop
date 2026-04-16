# mindmeld

Peer-to-peer terminal sharing over [iroh](https://iroh.computer). One person hosts a shell; others connect read-only and can request edit access. No relay servers to run, no accounts, no ports to open — connections are established directly between peers via iroh's QUIC transport.

## Install

```sh
cargo install mindmeld
```

This installs the `meld` binary.

## Usage

**Host a session** (shares your `$SHELL` by default; runs the given command otherwise):

```sh
meld host                  # shares $SHELL
meld host claude           # shares a specific command
```

On startup, the viewer command is copied to your clipboard:

```
meld view <ticket>
```

Send that to whoever should join.

**Join a session:**

```sh
meld view <ticket>
```

Viewers start read-only.

## Keybindings

### Host

| Key            | Action                                          |
| -------------- | ----------------------------------------------- |
| F9             | Accept a pending edit request / revoke the turn |
| F10            | Deny a pending edit request                     |
| Shift+PageUp   | Scroll back                                     |
| Shift+PageDown | Scroll forward                                  |

While no viewer holds the turn, the host types into the shared shell normally.

### Viewer

| Key      | Action                                           |
| -------- | ------------------------------------------------ |
| q        | Quit                                             |
| F9       | Request edit access / cancel request / release   |
| PageUp   | Scroll back                                      |
| PageDown | Scroll forward                                   |

Once the host grants the turn, the viewer's keystrokes flow into the shared shell. Host F9 revokes.

## Config

`~/.meld/config.toml` stores your display name, prompted for on first run:

```toml
name = "alice"
```

## Subprocess integration

Meld exports two hooks so subprocesses inside the shared shell can attribute actions to the user currently driving the session.

**`$MELD_SESSION_ID`** is set in the child environment to a per-session UUID. Presence of this variable tells a subprocess it's running inside a meld-shared shell.

**`~/.meld/sessions/$MELD_SESSION_ID/active_user`** is a plain-text file holding the display name of whoever currently holds the edit turn (the host, or the viewer who was granted by F9). It's rewritten atomically on every turn change and deleted when the session ends.

## How it works

The host spawns a PTY and binds an iroh endpoint with the `meld/term/0` ALPN. Viewers dial the host's ticket, open a bidirectional QUIC stream, and receive a keyframe replay of the current screen followed by a live stream of PTY output. A single viewer at a time may hold the edit turn; their keystrokes are forwarded to the host's PTY. The PTY is resized to the smallest connected viewport so everyone sees the same layout.

Wire protocol: `[tag: u8][len: u32 BE][payload]`. See `src/protocol.rs`.
