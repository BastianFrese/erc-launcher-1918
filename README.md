# ERC Launcher for F1 25

The launcher of **Erdi's Racing Community**: it starts F1 25 with the league's mods, compares the
mods before anyone joins, and opens lobbies on our own server instead of a third-party one.

Built on [Swofty Launcher](https://github.com/Swofty-Developments/SwoftyLauncher) by Jacob Nardella
(Swofty). Upstream arrives as release tags (`v1.9.x`); our own work sits on top of them. The upstream
parts up to v1.9.61 are under the GNU Affero General Public License v3 — see [LICENSE-AGPL](LICENSE-AGPL);
from v1.9.62 upstream is under the PolyForm Noncommercial License 1.0.0 — see [LICENSE](LICENSE).

## What the league added

### Lobby on our own relay

The **Multiplayer** page opens a room and hands out a four-character code; everyone else types that
code in and joins. No account, no sign-in, no open port at home. The room server
(`server/raum/raum-server.py`) only issues codes, room keys and member numbers — the race packets
themselves travel over the relay (`server/relay/erc-relay.py`, UDP `f1.erdi-erc.de:35501`), which
knows a room only by the code its packets carry. Room content is sealed with a key the server hands
out, so nobody outside the room can read it.

### Mod comparison before joining

The host sends a fingerprint of everything it runs: addons, versions, tracks, data packages. The
launcher turns that into work — missing or mismatched mods are fetched from the catalog, mods the
host does not run are switched off for that profile. If a difference survives, joining does not
happen and the launcher names the open point instead of letting the session fail later. See
`apps/launcher/src/abgleich.rs` and `packages/core/src/multiplayer/fingerprint.rs`.

### Player counter

Every launcher sends a heartbeat to the room server once a minute; the server counts the distinct
machines seen in the last five minutes and says how many of them are in the game. The start page
shows it as one line, and the league mod shows the same number as a race HUD toast. See
`apps/launcher/src/presence.rs`, `POST/GET /v1/presence` in the room server, and `presence.json`
next to the launcher's data.

### Updates

At start the launcher asks `download.erdi-erc.de/updates.json` for a newer version, verifies the
SHA-256 of the setup before running it, and replaces itself. The button in the title bar stays short;
the full changelog lives on the start page.

### League mod

`dist-1918/addons/erdi.erc-liga` connects the race control's panel to the game: a small text file
becomes a race HUD notification (flags, penalties, race-control messages). It is our own addon and
is not published separately.

### German by default

The launcher starts in German; other languages remain in the settings.

## The servers behind it

| Server | Where | What |
|---|---|---|
| Room server | `server/raum/`, LXC 121, `raum.erdi-erc.de` | room codes, room keys, member numbers, player counter |
| Relay | `server/relay/`, UDP `f1.erdi-erc.de:35501` | carries the race packets; the room list answers `ERC-LOBBYS` |
| Protocol | `server/protokoll/`, LXC 121 | takes session reports from the league's machines |

All three are plain Python 3 with the standard library only.

## Everything else comes from upstream

- **Addons:** anything with a `swofty.json`, installed on the Addons page. Data addons change the game
  database, text, `.erp` archives and menus; code addons edit the F1 25 engine directly, from new
  menus and settings to bigger grids.
- **Mods:** `.serps` packages, `.erp` archives and other game files in a zip, 7z, RAR or folder, plus
  community tracks.
- **DB Editor:** change drivers, teams, line-ups, tracks, the calendar, weather and points, saved as
  packages you can stack and share.
- **Profiles:** keep separate setups; the game folder goes back to vanilla when a session ends.
- **In-game addon settings:** under **ADDON SETTINGS** in the game's main menu.
- **ERC UI:** view and edit the game's menu layouts (`.idf` scenes) and ship the edits in an addon.

## Install

Download `ERC-Launcher-<version>-setup.exe` from **https://download.erdi-erc.de** and run it. It
installs per user under `%LOCALAPPDATA%\Programs\ERCL`, no administrator rights needed, and keeps
itself up to date from then on.

## Supported platforms

- Windows 10 and 11, with F1 25 from **Steam** or the **EA App** — the core ships the build
  signatures for both (four F1 25 builds at the time of writing). An unknown game build loads
  neither the core nor any addon, and the loader says so in its log.
- Linux with Steam and Proton is inherited from upstream and not tested by the league.

## Building

```bash
cargo test -p erc-launcher          # launcher tests
cargo build --release               # builds erc-launcher.exe and swofty.exe
native/build.sh                     # core DLLs; needs zig 0.16.0 on PATH
makensis packaging/erc-launcher.nsi # the installer
```

`native/build-win.sh` is behind `native/build.sh` — use `build.sh`. It regenerates the binding sets
and the supported-build table as part of the build.

## Source of the released installers

| Installer | Tag |
|---|---|
| `ERC-Launcher-1.3.3-setup.exe` | [`erc-1.3.3`](../../releases/tag/erc-1.3.3) |
| `ERC-Launcher-1.3.2-setup.exe` | [`erc-1.3.2`](../../releases/tag/erc-1.3.2) |
| `ERC-Launcher-1.3.1-setup.exe` | [`erc-1.3.1`](../../releases/tag/erc-1.3.1) |
| `ERC-Launcher-1.3.0-setup.exe` | [`erc-1.3.0`](../../releases/tag/erc-1.3.0) |
| `ERC-Launcher-1.2.2-setup.exe` | [`erc-1.2.2`](../../releases/tag/erc-1.2.2) |
| `ERC-Launcher-1.2.1-setup.exe` | [`erc-1.2.1`](../../releases/tag/erc-1.2.1) |
| `ERC-Launcher-1.3.0-beta.1-setup.exe` (test build) | [`erc-1.3.0-beta.1`](../../releases/tag/erc-1.3.0-beta.1) |

The 1.2.0 installer is superseded; its source is available on request. Every commit after the first
describes one change against the 1.2.1 state.

A build whose version carries a suffix (`1.3.0-beta.1`) is a test build: it reads
`updates-beta.json` instead of `updates.json`, so it only moves between test builds, and its
window title says "Beta". It installs into the same folder and therefore replaces the stable build.

**Order for a new version:** bump the version, build the installer, commit and push here, create the
release with the installer attached, and only then update the download page — so the page always
follows a release that already exists in git.
