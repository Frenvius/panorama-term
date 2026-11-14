# Remote desktop

*Leia em [português](REMOTE.pt-BR.md).*

Remote desktop lets another computer show and control a Panorama workspace over the
local network, Tailscale or the public relay. The host runs the terminals and holds the workspace,
files and Git repositories. Keep Panorama open on the host.

## Pair a desktop

1. On the host, open **Settings → Mobile**, turn on **Allow device connections** and
   choose **Pair a desktop**. Copy the entire desktop pairing URI.
2. On the other computer, open **Settings → Remote host**, paste it into **Pairing URI**
   and choose **Pair and connect**.

The pairing URI grants full access to the host; share it securely. To revoke a desktop,
remove it from **Settings → Mobile → Paired devices** on the host.

## How it works

The workspace, terminals, files and Git operations belong to the host. Each screen
keeps its own canvas camera position; appearance settings and plugins are local too.
Concurrent workspace changes use the last saved snapshot.

Connections use X25519 and XSalsa20-Poly1305 encryption. The host's public key is
pinned during pairing and checked on connection. A paired desktop has full control:
it can run commands and read and change files and Git repositories as the host user.
Panorama reconnects automatically if the connection is lost.

## Limits

- When local addresses are unavailable, paired desktops can connect through the public relay. Pairing still requires a local network connection.
- The client most recently used for input or focus controls the PTY size. Other clients adapt to that size.
