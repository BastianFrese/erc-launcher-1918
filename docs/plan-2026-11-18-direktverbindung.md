---
datum: 2026-11-18
tags: [erc-launcher, plan]
status: [laufend]
---

# Plan: Direktverbindung statt Relay (Peer-Adressen über den Relay)

## Ziel

Die Spielpakete sollen den Weg über Bastians Leitung verlassen, wo es geht. Der Relay
bleibt als Vermittler und als Rückfall, trägt aber im Regelfall keinen Spielverkehr
mehr.

Warum: gemessen am 18.11. kostet ein voller 9er-Raum ≈ 11 Mbit/s Upload, die Leitung
hat 42 Mbit/s — also etwa **drei bis vier volle Räume**, dann ist Schluss. Der Relay
selbst langweilt sich dabei (≈ 5 % eines Kerns, 5 MB RAM).

## Ausgangslage im Code (nachgesehen 18.11.)

| Punkt | Befund | Quelle |
|---|---|---|
| Gäste bekommen beim Beitritt keine Adresse | `(Some(relay_adresse()), Vec::new())` | `src-tauri/src/main.rs:3058` |
| Der Relay kennt Kennung → Quelladresse | `raeume[raum][kennung] = (quelle, zeit)` | `server/relay/erc-relay.py` |
| Umschlag ist für beide Wege gleich | `ERC-RELAY` + Raum(4) + Kennung(1) + Nutzlast | `main.rs:2273-2283` |
| Alte Launcher verwerfen Fremdes | im Relay-Betrieb nur Pakete mit `ERC-RELAY` — alles andere `continue` | `main.rs:~2320` |
| Mitgliedsnummern vergibt der Host | `vom_host`, PING/PONG | `main.rs:3055`, `nummer_fuer` |
| Adressen stehen an sechs Stellen im Protokoll | u. a. „Mitglied N über …", „Gegenüber N gemerkt (…)" | `main.rs:2165, 2338, 2351` |
| Puffer/Last | 212992 Byte Empfangspuffer (Kern kappt), 99,9 % Zustellung im Betrieb | eigene Messung 18.11. |

## Die drei Wege

**A — Relay als Adressvermittler, Launcher locht direkt** *(gewählt)*
Der Relay gibt jedem Mitglied die öffentliche Adresse der anderen. Der Launcher schickt
gleichzeitig ein Prüfpaket direkt und bleibt bis zur Antwort am Relay. Nur bestätigte
Gegenüber werden umgestellt. Aufwand ≈ 2 Tage, Serverlast fällt für passende Paare weg.

**B — A plus echter STUN-Server**
Zusätzlich das NAT-Verhalten vorab bestimmen (RFC 5780; der Codec liegt in
`multiplayer/stun.rs`). Weniger vergebliche Versuche, bessere Diagnose — ein Tag mehr
und ein zweiter Dienst, der laufen muss. Nicht gewählt, weil A auch ohne Vorprüfung
sauber zurückfällt.

**C — Relay in ein Rechenzentrum**
Gemieteter Server mit 1 Gbit, Launcher unverändert, 30–60 Minuten. Löst das Problem
sofort, hält aber die Abhängigkeit von einem fremden Rechner und lässt die IPs dort
sichtbar. Nicht gewählt: der Eigenbau ist der Punkt des Projekts.

## Was gebaut wird

### 1. Relay: Peer-Tabelle (`server/relay/erc-relay.py`)

Neue Steuernachricht **Client → Relay**:

```
"ERC-PEERS" + Raum(4 ASCII) + Kennung(1)
```

Antwort **Relay → Client**, nur wenn die Quelladresse schon Mitglied dieses Raums ist:

```
"ERC-PEERS" + Raum(4) + "\n" + je Zeile "k=adresse:port" (alle außer dem Fragenden)
```

- **Nur auf Anfrage** — der Relay schickt nichts von sich aus. Der Launcher fragt im
  Lobby-Takt (alle 10 s). Ein Anstoßen bei Mitgliederwechsel spart sechs Pakete je
  Minute und kostet einen zweiten Codeweg; kommt erst, wenn das Warten stört.
- Die Anfrage muss **vom selben Socket** kommen wie die Spielpakete (der Brücke auf
  35500) — sonst steht die Quelladresse nicht als Mitglied im Raum und die Antwort
  bleibt aus.
- **Nur brauchbare Adressen:** Einträge mit privater oder nicht-globaler Adresse
  (`10/8`, `172.16/12`, `192.168/16`, `127/8`, `169.254/16`, IPv6 nicht global) werden
  **weggelassen**. Sonst steht für einen Teilnehmer aus dem eigenen Netz `192.168.100.x`
  in der Tabelle, und jeder Gast locht still ins Leere.
- **Schutz:** Antwort nur an Adressen, die im Raum schon eingetragen sind.
  *Ehrlich dazu:* das ist schwach — ein einziges `ERC-RELAY`-Paket macht jeden zum
  Mitglied (so prüft der Launcher selbst die Verbindung). Wer den Raumcode hat, kann
  heute schon Pakete einspeisen; er erfährt künftig auch die Adressen. Bewusst
  angenommen, nicht weggeredet.
- Alte Launcher sind unberührt: sie verwerfen alles, was nicht mit `ERC-RELAY` beginnt.

### 2. Launcher: Lochen, Umschalten, Rückfall (`src-tauri/src/main.rs`)

- **Steuerung bleibt immer am Relay.** `senden()` wird **nicht** umgeschaltet, sondern
  ergänzt: Lobby-Meldung `ERC-RAUM`, die `ERC-PEERS`-Anfrage und Prüfrufe an Gegenüber
  ohne bestätigten Direktweg gehen weiterhin zum Relay. Nur **Spielpakete** werden je
  Gegenüber geroutet.
- **Reihenfolge ist Pflicht: erst Tabelle, dann Lochen.** Ohne Tabelle wird nicht
  gelocht und keine direkte Antwort gegeben.
- **Empfangen:** Pakete mit `ERC-PEERS` werden ausgewertet und **nie** ans Spiel
  weitergereicht. Alles andere wie bisher.
- **Lochen:** je Gegenüber aus der Tabelle 15 Pakete `ERC-PING <name>` im Abstand von
  200 ms direkt an dessen Adresse. Nach 3 s aufgeben, neuer Versuch nach 60 s.
- **Antworten an die Quelle:** ein `ERC-PING` aus einer Nicht-Relay-Adresse wird
  **direkt an die Quelladresse** beantwortet (heute geht die Antwort durch `senden()`
  zum Relay — ohne diese Änderung kommt kein Lochen zustande).
  **Nur** wenn die Quelladresse in der aktuellen Tabelle steht — sonst wäre der
  Launcher ein offener Antwortreflektor für jeden Scanner.
- **Die Tabelle ist ein Hinweis, kein Beweis.** Der Beitritts-Prüfruf läuft über einen
  Wegwerf-Socket mit fester Kennung 2 (`main.rs:2053`); dadurch kann der Relay kurzzeitig
  eine tote Adresse für Mitglied 2 führen, und zwei Gäste überschreiben sich. Das ist
  unschädlich, **weil nur ein empfangenes `ERC-PONG` umschaltet** — eine falsche Adresse
  kostet höchstens einen vergeblichen Lochversuch.
- **Umstellen:** je Gegenüber ein eigener Zustand `direkt(adresse)` oder `relay`.
  Umgestellt wird nur nach einem direkt empfangenen `ERC-PONG` oder einem gültigen
  Paket von genau der Adresse, die in der Tabelle steht. Alles andere: Relay.
- **Lebenszeichen:** 10 s Stille zu einem direkten Gegenüber → ein `ERC-PING` direkt.
  Drei Fehlschläge (30 s) → zurück auf Relay, Lochen nach 60 s erneut.
- **Spoofing-Schutz:** eine Adresse wird nur übernommen, wenn sie aus der Relay-Tabelle
  stammt oder die Antwort auf ein selbst gestartetes Lochen ist.
- **Schalter:** `ERC_DIREKT=0` ⇒ exakt heutiges Verhalten (alles über den Relay).

### 3. Maskierung der Adressen

- Ein Helfer `versteckt(addr)` → `79.245.•.•:63807` (IPv4, letzte zwei Stellen), bei
  IPv6 ebenso die letzten beiden Blöcke. Port bleibt: er verrät niemanden und hilft beim
  Fehlersuchen.
- Angewendet an den Stellen mit **fremden** Adressen (`main.rs`: „Mitglied N über …",
  „Adresse … ließ sich nicht auflösen", „Prüfruf von …", „Gegenüber N gemerkt …",
  „Brücke über …"). Die Adresse des **eigenen Relays** bleibt stehen — sie ist ein
  Dienstname, kein Mensch, und ohne sie ist keine Fehlersuche möglich.
  Damit ist die Maskierung **an der Quelle**, jede Weitergabe der Konsole ist schon
  gekürzt. Wichtig für
  `lauf_schicken` (`main.rs:1303`): die Konsole geht live an `protokoll.erdi-erc.de`,
  nicht erst im Bündel. *(Randnotiz: `protokoll_senden` (`main.rs:1219`) überschreibt
  seinen Parameter `zeilen` mit dem Spiellog — der Abschnitt „Konsole des Fensters" im
  Bündel enthält deshalb Spiellogzeilen. Eigener Fehler, nicht Teil dieses Plans.)*
- Schalter `ERC_ADRESSE_KLAR=1` zeigt sie vollständig.
- **Ehrlich:** das ist Kosmetik. Wer die Verbindung führt, sieht die Adresse im
  Betriebssystem (`netstat`, Wireshark). Die Maskierung nimmt den Zufallsblick und die
  weitergegebenen Protokolle, nicht den Zugriff.

## Was ausdrücklich nicht dazugehört

- Kein STUN-Server, keine Prüfung des NAT-Verhaltens (Weg B).
- Keine Änderung am TURN-Weg des Kerns (`swofty/packages/core`, `session.rs`, `turn.rs`).
- Keine Änderung am Raumserver (`/opt/raum/raum-server.py`) und keine am Spiel.
- Keine neuen Abhängigkeiten im Relay (weiter nur Standardbibliothek).
- Keine IPv6-Sonderbehandlung. Wenn beide Seiten IPv6 haben, klappt es nebenbei —
  versprochen wird es nicht.

## Grenzen und Risiken

| Risiko | Wirkung | Umgang |
|---|---|---|
| Symmetrisches NAT / CGNAT | Paar bleibt am Relay | kein Ausfall, nur keine Ersparnis |
| IP-Adressen sichtbar | Teilnehmer sehen sich gegenseitig | Maskierung in der Konsole; technisch unvermeidbar |
| Upload wandert zum Fahrer | voller Raum ≈ 1 Mbit/s je Fahrer | Schalter `ERC_DIREKT=0` je Person |
| Adress-Sammlung über Raumcode | Fremde erfahren Adressen | Relay antwortet nur an Mitglieder — **schwach**, ein Paket genügt (bewusst angenommen) |
| Spoofing einer Kennung | fremder Verkehr eingeschleust | Umschalten nur nach `ERC-PONG`; Antworten nur an Tabellenadressen |
| Offener Reflektor | Scanner nutzen den Launcher als Verstärker | PING wird nur Tabellenadressen beantwortet |
| Teilnehmer im eigenen Netz | `192.168.100.x` in der Tabelle, Lochversuch läuft ins Leere | private Adressen werden gar nicht erst ausgeliefert |
| Umschalten mitten im Rennen | wenige verlorene Pakete (~50 ms) | nur nach bestätigtem PONG, höchstens ein Wechsel je Gegenüber je 60 s |
| Alter Launcher als Gast | läuft weiter über Relay | keine Steuernachricht, kein Fehler |

## Testfälle

1. **Zwei Launcher auf zwei Gästen/VMs im selben Netz:** Tabelle kommt an, Lochen klappt,
   Umschalten sichtbar im Protokoll.
   *Nicht auf einem Rechner:* beide Brücken binden UDP 35500, der zweite scheitert mit
   „Port 35500 ließ sich nicht öffnen" (`main.rs:2219`).
2. **Drei Rechner in zwei Netzen:** Konsole zeigt je Gegenüber „direkt"; im
   Relay-Journal stehen für die Renndauer nur noch Lebenszeichen statt Spielpaketen.
3. **Mobilfunk-Hotspot als einer der drei:** dieser bleibt am Relay, die anderen beiden
   gehen direkt, das Rennen läuft unverändert.
4. **Alter Launcher als Gast:** Beitritt, Rennen und Mod-Abgleich wie heute; im
   Relay-Journal kein einziges Paket der neuen Steuerung von ihm.
5. **Direktweg gekappt** (Firewallregel auf einem Rechner): binnen 30 s zurück am Relay,
   Spiel ohne Abbruch.
6. **Schalter `ERC_DIREKT=0`:** Verhalten wie vor dem Umbau, Byte für Byte über den Relay.

## Was der Prüfbericht geändert hat (18.11.)

| Frage des Prüfers | Antwort im Plan |
|---|---|
| Sieht der Relay öffentliche Adressen? Ein Teilnehmer aus dem eigenen Netz erscheint als `192.168.100.x`. | Private/nicht-globale Adressen werden **weggelassen** — kein stiller Fehlversuch. Bastian selbst bleibt dann am Relay; seine Strecke ist ohnehin Netz-intern. |
| „Nur an Mitglieder" trägt nicht — ein Paket genügt. | **Bewusst angenommen** und im Risikoteil benannt, nicht als Schutz verkauft. |
| Beitritts-Prüfruf nutzt Wegwerf-Socket und feste Kennung 2 → Leichen in der Tabelle. | **Tabelle ist Hinweis, PONG ist Beweis.** Umschalten nur nach PONG; Lochen wird alle 60 s wiederholt. Kein Eingriff an der Prüfrufstelle. |
| Wer darf ein direktes PING beantworten? | Nur Adressen aus der aktuellen Tabelle — sonst offener Reflektor. |
| Zwei Launcher auf einem Rechner geht nicht (Port 35500 doppelt). | Test 1 wandert auf zwei Gäste/VMs. |
| Was bleibt zwingend am Relay? | Steuerung (Lobby, Tabelle, Prüfrufe ohne Direktweg) **immer**; nur Spielpakete je Gegenüber. `senden()` wird ergänzt, nicht umgeschaltet. |
| Trägt die Begründung der Maskierung? | Nein — die Belege gehen über `lauf_schicken` live raus, nicht erst im Bündel. Begründung im Plan korrigiert; dabei einen fremden Fehler gefunden (`protokoll_senden` überschreibt seine Konsole mit dem Spiellog). |

## Aufwand

### Stand 18.11.2026

- **Relay fertig und ausgeliefert** (18.11., 03:57Z): `ERC-PEERS`, Adressfilter,
  Mitgliedsprüfung, Selbsttest (`python3 erc-relay.py --selbsttest`). Live geprüft:
  Mitglied bekommt die Tabelle (leer, weil im Testnetz beide Adressen privat sind),
  ein Fremder bekommt nichts und es steht eine Zeile im Journal.
- **Maskierung fertig** (`versteckt()`, Test `adressen_werden_im_protokoll_gekuerzt`),
  noch nicht gebaut/ausgeliefert.
- **Lochen und Umschalten gebaut**, hinter `ERC_DIREKT=1` (Vorgabe: **aus**, also heute
  unverändertes Verhalten). `cargo check` und alle 18 Tests grün.
  - Tabelle wird alle 10 s beim Relay erfragt (`ERC-PEERS`).
  - Lochen: 15 Prüfrufe in 3 s, danach 60 s Pause.
  - Umgeschaltet wird nur, wenn ein Paket **von genau der Tabellenadresse** kommt.
  - Lebenszeichen: 10 s Stille → ein Prüfruf direkt, 30 s Stille → zurück über den Relay.
  - Pakete von Adressen, die weder Relay noch Tabelle sind, werden verworfen.
- **Noch nicht geprüft:** das Verhalten zu zweit über zwei echte Netze. Erst danach
  darf `ERC_DIREKT` in der Vorgabe auf `1`.

| Schritt | Zeit |
|---|---|
| Relay: Steuernachricht, Adressfilter, Mitgliedsprüfung, Zähler | ½ Tag |
| Launcher: Tabelle, Lochen, Umschalten, Lebenszeichen, Rückfall | 1 Tag |
| Maskierung | ¼ Tag |
| Tests 1–6, davon einer zu dritt über zwei Netze | ½ Tag |

Zusammen ≈ 2 Tage, davon der letzte nur mit echten Gegenübern sinnvoll.

## Reihenfolge

1. Relay zuerst — er allein ändert nichts am Verhalten (nur eine neue Nachricht).
2. Maskierung — unabhängig, sofort ausliefernswert.
3. Launcher-Lochen hinter `ERC_DIREKT` (Standard erst `0`, dann `1` nach dem Dritt-Test).

So kann nichts kaputtgehen, was nicht vorher grün war.

## Rückweg

- `ERC_DIREKT=0` — alter Weg, ohne neuen Build.
- `ERC_RELAY` — andere Relay-Adresse.
- Sicherungen: `/opt/relay/erc-relay.py.bak-20261118`,
  `/etc/pve/lxc/121.conf.bak-20261118`.
