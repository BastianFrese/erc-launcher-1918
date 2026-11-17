# Online mit Mods — Ansatz

Stand 2026-10-30. Grundlage: der Quellcode des Swofty-Launchers
(`Downloads\_swofty-src\SwoftyLauncher-main`), vom Swofty-Entwickler über Discord
überlassen.

## Was der Launcher enthält — und was nicht

| Teil | Vorhanden? |
|---|---|
| Loader (schaltet EA AntiCheat für die Sitzung ab) | ja, `native/` |
| Addon-API, Mixins, Datenbank-Editor | ja |
| Reverse-Engineering-Bestand (`re/`): 3321 Signaturen, Subsystem-Dokus, Traces | ja |
| Netzwerk- oder Sitzungs-Transport | **nein** — kein `SteamNetworking`, kein Session-Code in der Quelle |

Der Launcher ist also keine Grundlage für Multiplayer, sondern der **Schlüsselbund**:
er liefert die Namen, Adressen und Prototypen der Engine-Funktionen, die man dafür
braucht.

## Was im Spiel vorhanden ist

F1 25 hat einen vollständigen Sitzungsapparat mit Host- und Peer-Rollen, und darin
**einen LAN-Transport**:

```
core.net_transport_lan_register_arrived_connection
core.net_session_lan_migrate_session_host
game.network_transport_connect
game.network_transport_send_connect_message
game.network_transport_initialise_async_socket
game.game_session_manager_on_connection_arrived / _left
game.vehicle_authority_manager_on_host_migrated
```

601 Netzwerk- und Sitzungsnamen liegen in `docs/netzwerk-signaturen.txt`, die
zugehörigen RVA und Muster in `re/db/signatures.json`.

## Warum es trotzdem nicht geht

Die Swofty-Release-Notizen sagen es selbst (1.5.13): „**Local Multiplayer** goes straight
to Splitscreen. The LAN Game entry, **which can't host a game in a modded session**, is
hidden." Der LAN-Eintrag existiert also, aber Swofty blendet ihn aus, weil das Hosten in
einer modded Sitzung scheitert.

**Das ist die eigentliche Frage:** woran scheitert es? Kandidaten:
- der Content-Verifier (`ContentDomainVerifier`, `GameSessionManager_ContentDomainVerifier_SendTo`)
  — modded Daten weichen von der Erwartung ab
- die Plattformbindung (Steam/EA) beim Sitzungsaufbau
- fehlendes Anti-Cheat, das der Sitzungscode voraussetzt

## Nächster Schritt

Den LAN-Eintrag in einer modded Sitzung **sichtbar machen und auslösen**, statt ihn zu
verstecken: `Game_NetworkTransport_Connect` bzw. die LAN-Registrierung aus dem Plugin
aufrufen und im Log verfolgen, wo es abbricht. Dafür sind die Signaturen und die
Mixin-Mechanik bereits vorhanden — es fehlt nur die Zuordnung Name → Aufruf.
