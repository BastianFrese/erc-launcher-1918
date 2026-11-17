// ERC Liga bridge — reads the race control's command file and shows it in the race HUD.
//
// The panel (erc-steward-panel) writes one small text file; this plugin polls it once
// every half second and posts a HUD toast when the content changes. That is the whole
// first step: prove that the panel can reach the game at all. Triggering a real safety
// car needs engine functions that are not in the SDK, so it comes later.
//
// File: %LOCALAPPDATA%\Swofty\addon-data\erdi.erc-liga\befehl.txt
//   line 1: gruen | gelb | sc | vsc | rot | ziel
//   line 2: optional text of its own

#include <swofty/events.hpp>
#include <swofty/hud.hpp>
#include <swofty/mem.hpp>
#include <swofty/mixin.hpp>

#include <windows.h>

#include <cstdio>
#include <cstring>
#include <string>

namespace {

struct Flag {
    const char *token;
    const char *text;
    const char *icon;
};

// Icons the race HUD knows: see swofty/hud.hpp. Unknown names are refused by the core.
constexpr Flag kFlags[] = {
    {"gruen", "Strecke frei", "generic"},
    {"gelb", "Gelb — Achtung", "diagonal"},
    {"sc", "Safety Car", "safety_car"},
    {"vsc", "Virtuelles Safety Car", "virtual_safety_car"},
    {"rot", "Rennen unterbrochen", "red"},
    {"ziel", "Zielflagge", "chequered"},
};

std::string g_letzter_befehl;
double g_seit_pruefung = 0.0;
double g_seit_status = 0.0;
constexpr double kPruefintervall = 0.5;
constexpr double kStatusintervall = 2.0;

std::string datenordner()
{
    char pfad[MAX_PATH] = {};
    const DWORD n = GetEnvironmentVariableA("LOCALAPPDATA", pfad, MAX_PATH);
    if (n == 0 || n >= MAX_PATH)
        return {};
    std::string dir = std::string(pfad) + "\\Swofty\\addon-data\\erdi.erc-liga";
    CreateDirectoryA(dir.c_str(), nullptr);
    return dir;
}

std::string befehlsdatei()
{
    const std::string dir = datenordner();
    return dir.empty() ? std::string() : dir + "\\befehl.txt";
}

std::string lesen(const std::string &pfad)
{
    if (pfad.empty())
        return {};
    FILE *f = nullptr;
    if (fopen_s(&f, pfad.c_str(), "rb") != 0 || f == nullptr)
        return {};
    char puffer[256] = {};
    const size_t gelesen = fread(puffer, 1, sizeof(puffer) - 1, f);
    fclose(f);
    return std::string(puffer, gelesen);
}

const Flag *finden(const std::string &token)
{
    for (const Flag &f : kFlags)
        if (token == f.token)
            return &f;
    return nullptr;
}

void wiederholen();
void sc_debug_ausloesen();
void rotflagge_ausloesen();

// --- Wie viele gerade spielen ---------------------------------------------
//
// Der Launcher legt den Stand als %LOCALAPPDATA%\Swofty\presence.json ab und
// erneuert ihn im Minutentakt. Wir lesen ihn und sagen ihn im Renn-HUD an —
// hoechstens alle fuenf Minuten, damit es nicht nervt. Der Kern zeigt Toasts
// nur waehrend einer Sitzung auf der Strecke; in den Menues passiert nichts.

std::string g_letzter_anwesend;
ULONGLONG g_anwesend_wann = 0;
double g_seit_anwesend = 0.0;
constexpr double kAnwesendintervall = 60.0;
constexpr ULONGLONG kAnwesend_abstand = 300000ULL;

std::string presence_datei()
{
    char pfad[MAX_PATH] = {};
    const DWORD n = GetEnvironmentVariableA("LOCALAPPDATA", pfad, MAX_PATH);
    if (n == 0 || n >= MAX_PATH)
        return {};
    return std::string(pfad) + "\\Swofty\\presence.json";
}

// Leer, wenn die Datei fehlt, unlesbar ist oder niemand mehr da ist.
std::string anwesend_text()
{
    const std::string roh = lesen(presence_datei());
    if (roh.empty())
        return {};
    int launcher = 0;
    int ingame = 0;
    if (sscanf_s(roh.c_str(), "{\"launcher\":%d,\"ingame\":%d}", &launcher, &ingame) != 2)
        return {};
    if (launcher <= 0)
        return {};
    if (ingame <= 0)
        return std::to_string(launcher) + " im Launcher";
    return std::to_string(launcher) + " im Launcher - " + std::to_string(ingame) + " im Rennen";
}

void anwesend_melden()
{
    const std::string text = anwesend_text();
    if (text.empty() || text == g_letzter_anwesend)
        return;
    if (g_anwesend_wann != 0 && GetTickCount64() - g_anwesend_wann < kAnwesend_abstand)
        return;
    if (!swofty::hud::available())
        return;
    if (swofty::hud::toast(text.c_str(), "ERC Launcher", 3.0f) == swofty::hud::Result::ok) {
        g_letzter_anwesend = text;
        g_anwesend_wann = GetTickCount64();
    }
}

void melden(const std::string &roh)
{
    const size_t zeilenende = roh.find('\n');
    std::string token = roh.substr(0, zeilenende);
    while (!token.empty() && (token.back() == '\r' || token.back() == ' '))
        token.pop_back();

    // Ein Wiederholbefehl darf mehrfach hintereinander kommen - deshalb vor der
    // Gleichheitspruefung, die nur Flaggenwechsel entprellt.
    if (token == "warnung-wiederholen") {
        wiederholen();
        DeleteFileA(befehlsdatei().c_str()); // einmal ausfuehren, dann weg
        return;
    }
    if (token == "sc-debug") {
        sc_debug_ausloesen();
        DeleteFileA(befehlsdatei().c_str());
        return;
    }
    if (token == "rotflagge") {
        rotflagge_ausloesen();
        DeleteFileA(befehlsdatei().c_str());
        return;
    }

    if (roh == g_letzter_befehl)
        return;

    std::string eigener_text;
    if (zeilenende != std::string::npos)
        eigener_text = roh.substr(zeilenende + 1);

    if (token.empty())
        return;

    g_letzter_befehl = roh;

    const Flag *flagge = finden(token);
    if (flagge == nullptr) {
        swofty::log("[erc-liga] unbekannter Befehl: %s", token.c_str());
        return;
    }
    if (!swofty::hud::available()) {
        swofty::log("[erc-liga] HUD nicht verfuegbar auf diesem Build");
        return;
    }

    const std::string &text = eigener_text.empty() ? std::string(flagge->text) : eigener_text;
    const auto ergebnis = swofty::hud::toast(text.c_str(), "ERC Rennleitung", 6.0f, flagge->icon);
    swofty::log("[erc-liga] %s -> %.*s (%s)", flagge->token, int(eigener_text.size() > 60 ? 60 : eigener_text.size()),
                eigener_text.empty() ? flagge->text : eigener_text.c_str(),
                ergebnis == swofty::hud::Result::ok ? "ok" : "abgelehnt");
}

// Safety Car und rote Flagge: Prototypen stehen nicht in der Bindungsliste, also
// schreiben wir alle vier Argumentregister (rcx, rdx, r8, r9) mit. Bei x64 ist das
// gefahrlos: eine Funktion, die weniger Argumente hat, ignoriert die ueberzaehligen.
struct ScBeobachtung {
    void *obj = nullptr;
    uint64_t a2 = 0, a3 = 0, a4 = 0;
    int anzahl = 0;
    ULONGLONG wann = 0;
};

ScBeobachtung g_sc, g_rotflagge;
void *g_orig_sc = nullptr;
void *g_orig_rotflagge = nullptr;

using GrobFn = void (*)(void *, uint64_t, uint64_t, uint64_t);
void *g_orig_sc_logic = nullptr;
void *g_orig_vsc_cond = nullptr;
void *g_orig_rot_beginn = nullptr;

// Safety-Car- und Rotflaggen-Objekt: nur aus Funktionen, die zum System gehoeren.
// "OnLapChanged" scheidet aus - das feuert pro Fahrzeug (neun Aufrufe, neun Zeiger)
// und hat beim ersten Versuch zum Absturz gefuehrt.
void *g_sc_objekt = nullptr;
void *g_rot_objekt = nullptr;

bool glaubwuerdig(void *kandidat)
{
    // Ein C++-Objekt des Spiels beginnt mit einem Zeiger auf seine Vtable im Abbild.
    return kandidat != nullptr && swofty::mem::object_like(kandidat);
}

void merke_objekt(void *kandidat, void *&ziel, const char *wo)
{
    if (ziel != nullptr || !glaubwuerdig(kandidat))
        return;
    ziel = kandidat;
    swofty::log("[erc-liga] Objekt gemerkt aus %s: %p", wo, kandidat);
}

void h_sc_logic(void *obj, uint64_t a2, uint64_t a3, uint64_t a4)
{
    merke_objekt(obj, g_sc_objekt, "SafetyCarLogic_Subscribe");
    if (g_orig_sc_logic)
        reinterpret_cast<GrobFn>(g_orig_sc_logic)(obj, a2, a3, a4);
}

// Laeuft im Rennen staendig - liefert das SC-Objekt, ohne dass ein Safety Car kommen muss.
void h_vsc_cond(void *obj, uint64_t a2, uint64_t a3, uint64_t a4)
{
    merke_objekt(obj, g_sc_objekt, "ScCond_TriggerVsc_Update");
    if (g_orig_vsc_cond)
        reinterpret_cast<GrobFn>(g_orig_vsc_cond)(obj, a2, a3, a4);
}

// Laeuft beim Sitzungsbeginn - liefert das Rotflaggen-System ohne rote Flagge.
void h_rot_beginn(void *obj, uint64_t a2, uint64_t a3, uint64_t a4)
{
    merke_objekt(obj, g_rot_objekt, "RedFlagSystem_OnSessionBeginGameplay");
    if (g_orig_rot_beginn)
        reinterpret_cast<GrobFn>(g_orig_rot_beginn)(obj, a2, a3, a4);
}

// Ruft einen Vtable-Eintrag mit dem gemerkten Objekt auf - mit Selbstpruefung.
void vtable_rufen(const char *slot_id, void *obj, const char *name)
{
    const uintptr_t slot = swofty::api->resolve(slot_id);
    if (slot == 0 || !swofty::mem::readable(slot, sizeof(void *))) {
        swofty::log("[erc-liga] %s: Slot %p nicht lesbar", name, reinterpret_cast<void *>(slot));
        return;
    }
    const uintptr_t fn = *reinterpret_cast<uintptr_t *>(slot);
    if (!swofty::mem::in_image(fn)) {
        swofty::log("[erc-liga] %s: Funktion %p liegt nicht im Abbild", name, reinterpret_cast<void *>(fn));
        return;
    }
    if (!glaubwuerdig(obj)) {
        swofty::log("[erc-liga] %s: kein brauchbares Objekt (%p)", name, obj);
        return;
    }
    swofty::log("[erc-liga] %s: Aufruf Funktion %p mit obj=%p", name, reinterpret_cast<void *>(fn), obj);
    reinterpret_cast<GrobFn>(fn)(obj, 0, 0, 0);
    swofty::log("[erc-liga] %s: Aufruf durch", name);
}

void sc_debug_ausloesen() { vtable_rufen("vt_Sess_ScDebugDeploy", g_sc_objekt, "sc-debug"); }
void rotflagge_ausloesen() { vtable_rufen("vt_Sess_ScTriggerRedFlag", g_rot_objekt, "rotflagge"); }

using GrobFn = void (*)(void *, uint64_t, uint64_t, uint64_t);

void h_sc_zustand(void *obj, uint64_t a2, uint64_t a3, uint64_t a4)
{
    g_sc = {obj, a2, a3, a4, g_sc.anzahl + 1, GetTickCount64()};
    swofty::log("[erc-liga] BEOBACHTET SafetyCar-Zustand: obj=%p rdx=%llu r8=%llu r9=%llu", obj,
                (unsigned long long)a2, (unsigned long long)a3, (unsigned long long)a4);
    if (g_orig_sc)
        reinterpret_cast<GrobFn>(g_orig_sc)(obj, a2, a3, a4);
}

void h_rotflagge(void *obj, uint64_t a2, uint64_t a3, uint64_t a4)
{
    g_rotflagge = {obj, a2, a3, a4, g_rotflagge.anzahl + 1, GetTickCount64()};
    swofty::log("[erc-liga] BEOBACHTET Rotflagge-Deploy: obj=%p rdx=%llu r8=%llu r9=%llu", obj,
                (unsigned long long)a2, (unsigned long long)a3, (unsigned long long)a4);
    if (g_orig_rotflagge)
        reinterpret_cast<GrobFn>(g_orig_rotflagge)(obj, a2, a3, a4);
}

// --- Netzwerk-Spione ------------------------------------------------------
//
// Frage: wird der LAN-/Sitzungsweg in einer modded Sitzung ueberhaupt betreten?
// Diese Spione rufen das Original immer durch und protokollieren nur. Sie aendern
// nichts - sie sagen uns, welcher der drei Verdaechtigen (Content-Verifier,
// Plattformbindung, fehlendes Anti-Cheat) den Ausschlag gibt.

struct NetZaehler {
    const char *name;
    int anzahl = 0;
};

NetZaehler g_net_connect, g_net_angekommen, g_net_migration, g_net_startlights;

void net_melden(NetZaehler &z, void *a, uint64_t b, uint64_t c, uint64_t d)
{
    z.anzahl++;
    if (z.anzahl <= 5 || z.anzahl % 50 == 0)
        swofty::log("[erc-liga] NETZ %s #%d: this=%p rdx=%llu r8=%llu r9=%llu", z.name, z.anzahl, a,
                    (unsigned long long)b, (unsigned long long)c, (unsigned long long)d);
}

void *g_orig_net_connect = nullptr;
void *g_orig_net_angekommen = nullptr;
void *g_orig_net_migration = nullptr;
void *g_orig_net_startlights = nullptr;

void h_net_connect(void *a, uint64_t b, uint64_t c, uint64_t d)
{
    net_melden(g_net_connect, a, b, c, d);
    if (g_orig_net_connect)
        reinterpret_cast<GrobFn>(g_orig_net_connect)(a, b, c, d);
}

void h_net_angekommen(void *a, uint64_t b, uint64_t c, uint64_t d)
{
    net_melden(g_net_angekommen, a, b, c, d);
    if (g_orig_net_angekommen)
        reinterpret_cast<GrobFn>(g_orig_net_angekommen)(a, b, c, d);
}

void h_net_migration(void *a, uint64_t b, uint64_t c, uint64_t d)
{
    net_melden(g_net_migration, a, b, c, d);
    if (g_orig_net_migration)
        reinterpret_cast<GrobFn>(g_orig_net_migration)(a, b, c, d);
}

void h_net_startlights(void *a, uint64_t b, uint64_t c, uint64_t d)
{
    net_melden(g_net_startlights, a, b, c, d);
    if (g_orig_net_startlights)
        reinterpret_cast<GrobFn>(g_orig_net_startlights)(a, b, c, d);
}

// --- LAN-Kachel zurueckholen -------------------------------------------------
//
// Der Kern springt beim Aufbau des Local-Multiplayer-Menues ueber die LAN-Kachel
// (E9 <rel32> 90 90 90 90 an RVA 0x173e7a7, Signatur "FeTile_LocalMp_LanTile").
// Die ausfuehrbare Datei auf der Platte bleibt dabei unveraendert - Swofty patcht nur
// im Speicher. Also lesen wir die Originalbytes aus der Datei und schreiben sie zurueck.
//
// Nur der bekannte Build wird angefasst: steht an der Stelle kein Sprung, passiert nichts.

constexpr uintptr_t kKachelRva = 0x173e7a7;
constexpr size_t kKachelBytes = 9;

bool bild_offset(uintptr_t rva, uint32_t &datei_offset)
{
    auto *basis = reinterpret_cast<uint8_t *>(swofty::api->module_base());
    if (!basis)
        return false;
    auto *dos = reinterpret_cast<IMAGE_DOS_HEADER *>(basis);
    if (dos->e_magic != IMAGE_DOS_SIGNATURE)
        return false;
    auto *nt = reinterpret_cast<IMAGE_NT_HEADERS *>(basis + dos->e_lfanew);
    if (nt->Signature != IMAGE_NT_SIGNATURE)
        return false;
    auto *abschnitt = IMAGE_FIRST_SECTION(nt);
    for (unsigned i = 0; i < nt->FileHeader.NumberOfSections; i++) {
        const uintptr_t start = abschnitt[i].VirtualAddress;
        const uintptr_t groesse = abschnitt[i].Misc.VirtualSize;
        if (rva >= start && rva < start + groesse) {
            datei_offset = abschnitt[i].PointerToRawData + uint32_t(rva - start);
            return true;
        }
    }
    return false;
}

bool kachel_zurueckholen()
{
    auto *stelle = reinterpret_cast<uint8_t *>(swofty::api->module_base() + kKachelRva);

    bool ist_gepatcht = stelle[0] == 0xE9 && stelle[5] == 0x90 && stelle[8] == 0x90;
    if (!ist_gepatcht)
        return false;

    char exe[MAX_PATH] = {};
    if (GetModuleFileNameA(nullptr, exe, MAX_PATH) == 0) {
        swofty::log("[erc-liga] Kachel: Pfad der Spieldatei nicht lesbar");
        return false;
    }

    uint32_t offset = 0;
    if (!bild_offset(kKachelRva, offset)) {
        swofty::log("[erc-liga] Kachel: RVA %p liegt in keinem Abschnitt", reinterpret_cast<void *>(kKachelRva));
        return false;
    }

    uint8_t original[kKachelBytes] = {};
    FILE *f = nullptr;
    if (fopen_s(&f, exe, "rb") != 0 || f == nullptr) {
        swofty::log("[erc-liga] Kachel: Spieldatei nicht lesbar: %s", exe);
        return false;
    }
    fseek(f, long(offset), SEEK_SET);
    const size_t gelesen = fread(original, 1, sizeof(original), f);
    fclose(f);
    if (gelesen != sizeof(original) || original[0] != 0x4C) {
        swofty::log("[erc-liga] Kachel: Originalbytes unplausibel (%zu gelesen, erstes %02X)", gelesen, original[0]);
        return false;
    }

    DWORD alt = 0;
    if (!VirtualProtect(stelle, kKachelBytes, PAGE_EXECUTE_READWRITE, &alt)) {
        swofty::log("[erc-liga] Kachel: Speicher nicht schreibbar (Fehler %lu)", GetLastError());
        return false;
    }
    memcpy(stelle, original, kKachelBytes);
    VirtualProtect(stelle, kKachelBytes, alt, &alt);
    FlushInstructionCache(GetCurrentProcess(), stelle, kKachelBytes);
    swofty::log("[erc-liga] Kachel zurueckgeholt: %02X %02X %02X %02X %02X %02X %02X %02X %02X", stelle[0], stelle[1],
                stelle[2], stelle[3], stelle[4], stelle[5], stelle[6], stelle[7], stelle[8]);
    return true;
}

void status_schreiben();

int g_kachel_versuche = 0;
bool g_kachel_fertig = false;

void bei_frame(double dt, void *)
{
    // Der Kern setzt seine Kachel-Sperre beim eigenen Setup - mal vor, mal nach unserem
    // Laden. Deshalb ein paar Sekunden lang nachsehen, statt einmal zu pruefen.
    if (!g_kachel_fertig && g_kachel_versuche < 60) {
        g_kachel_versuche++;
        if (kachel_zurueckholen()) {
            g_kachel_fertig = true;
        } else if (g_kachel_versuche == 12) {
            swofty::log("[erc-liga] Kachel: nach 6 s kein Sprung an RVA %p - Build anders?",
                        reinterpret_cast<void *>(kKachelRva));
        }
    }
    g_seit_status += dt;
    if (g_seit_status >= kStatusintervall) {
        g_seit_status = 0.0;
        status_schreiben();
    }
    g_seit_anwesend += dt;
    if (g_seit_anwesend >= kAnwesendintervall) {
        g_seit_anwesend = 0.0;
        anwesend_melden();
    }
    g_seit_pruefung += dt;
    if (g_seit_pruefung < kPruefintervall)
        return;
    g_seit_pruefung = 0.0;
    melden(lesen(befehlsdatei()));
}

// --- Spion: was die Engine selbst tut -------------------------------------
//
// Die Prototypen stammen aus swofty/addons/core/bindings.json (Sess_Issue_*).
// Wir haengen uns daneben, rufen das Original durch und schreiben mit, welche
// Zeiger die Engine benutzt. Damit kennen wir die Objekte, ohne zu raten.

struct Beobachtung {
    void *rules = nullptr;
    uint8_t *incident = nullptr;
    uint32_t reason = 0;
    uint32_t seconds = 0;
    int anzahl = 0;
    ULONGLONG wann = 0; // GetTickCount64 beim letzten Aufruf
};

Beobachtung g_warnung, g_zeitstrafe, g_durchfahrt;

// Original-Zeiger, die register_mixins zurueckgibt.
void *g_orig_warnung = nullptr;
void *g_orig_zeitstrafe = nullptr;
void *g_orig_durchfahrt = nullptr;

using IssueFn = void (*)(void *, uint8_t *, uint32_t);
using ZeitFn = void (*)(void *, uint8_t *, uint32_t, uint32_t);

std::string statusdatei()
{
    const std::string dir = datenordner();
    return dir.empty() ? std::string() : dir + "\\status.txt";
}

void status_schreiben()
{
    const std::string pfad = statusdatei();
    if (pfad.empty())
        return;
    FILE *f = nullptr;
    if (fopen_s(&f, pfad.c_str(), "wb") != 0 || f == nullptr)
        return;
    fprintf(f, "warnung=%d\nzeitstrafe=%d\ndurchfahrt=%d\nsc=%d\nrotflagge=%d\n", g_warnung.anzahl,
            g_zeitstrafe.anzahl, g_durchfahrt.anzahl, g_sc.anzahl, g_rotflagge.anzahl);
    fclose(f);
}

void h_warnung(void *rules, uint8_t *incident, uint32_t reason)
{
    g_warnung = {rules, incident, reason, 0, g_warnung.anzahl + 1, GetTickCount64()};
    swofty::log("[erc-liga] BEOBACHTET Warnung: rules=%p incident=%p reason=%u (0x%08X)", rules,
                static_cast<void *>(incident), reason, reason);
    // Erste Bytes des Incident-Puffers mitschreiben - darin steckt, worum es geht.
    unsigned char probe[16] = {};
    if (swofty::mem::readable(incident, sizeof(probe))) {
        memcpy(probe, incident, sizeof(probe));
        swofty::log("[erc-liga] incident[0..15]: %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X",
                    probe[0], probe[1], probe[2], probe[3], probe[4], probe[5], probe[6], probe[7], probe[8],
                    probe[9], probe[10], probe[11], probe[12], probe[13], probe[14], probe[15]);
    }
    if (g_orig_warnung)
        reinterpret_cast<IssueFn>(g_orig_warnung)(rules, incident, reason);
}

void h_zeitstrafe(void *rules, uint8_t *incident, uint32_t reason, uint32_t seconds)
{
    g_zeitstrafe = {rules, incident, reason, seconds, g_zeitstrafe.anzahl + 1, GetTickCount64()};
    swofty::log("[erc-liga] BEOBACHTET Zeitstrafe: rules=%p incident=%p reason=%u sekunden=%u", rules,
                static_cast<void *>(incident), reason, seconds);
    if (g_orig_zeitstrafe)
        reinterpret_cast<ZeitFn>(g_orig_zeitstrafe)(rules, incident, reason, seconds);
}

void h_durchfahrt(void *rules, uint8_t *incident, uint32_t reason)
{
    g_durchfahrt = {rules, incident, reason, 0, g_durchfahrt.anzahl + 1, GetTickCount64()};
    swofty::log("[erc-liga] BEOBACHTET Durchfahrt: rules=%p incident=%p reason=%u", rules,
                static_cast<void *>(incident), reason);
    if (g_orig_durchfahrt)
        reinterpret_cast<IssueFn>(g_orig_durchfahrt)(rules, incident, reason);
}

// Wiederholt den zuletzt gesehenen Aufruf mit genau den Zeigern, die die Engine selbst
// benutzt hat. Nur wenn die Zeiger noch lesbar sind und die Beobachtung frisch ist -
// ein Heap-Zeiger von vor zehn Minuten ist Muell und wuerde die Sitzung beenden.
void wiederholen()
{
    if (g_orig_warnung == nullptr) {
        swofty::log("[erc-liga] Wiederholen: kein Hook");
        return;
    }
    if (g_warnung.rules == nullptr || g_warnung.incident == nullptr) {
        swofty::log("[erc-liga] Wiederholen: noch keine Warnung beobachtet");
        return;
    }
    if (GetTickCount64() - g_warnung.wann > 180000ULL) {
        swofty::log("[erc-liga] Wiederholen: Beobachtung zu alt (%llu s)", (GetTickCount64() - g_warnung.wann) / 1000);
        return;
    }
    if (!swofty::mem::readable(g_warnung.rules, 8) || !swofty::mem::readable(g_warnung.incident, 1)) {
        swofty::log("[erc-liga] Wiederholen: Zeiger nicht mehr lesbar - abgebrochen");
        return;
    }
    swofty::log("[erc-liga] Wiederholen mit rules=%p incident=%p reason=%u", g_warnung.rules,
                static_cast<void *>(g_warnung.incident), g_warnung.reason);
    reinterpret_cast<IssueFn>(g_orig_warnung)(g_warnung.rules, g_warnung.incident, g_warnung.reason);
    swofty::log("[erc-liga] Wiederholen: Aufruf durch");
}

void spion_einhaengen()
{
    // Die Mixins selbst sind statisch registriert (siehe unten) — der Kern haengt sie im
    // SwoftyAddonInit ein, also vor ON_LOAD. Hier nur nachsehen, ob es geklappt hat.
    const char *namen[][2] = {
        {"Sess_Issue_Warning", "Warnung"},
        {"Sess_Issue_TimePenalty", "Zeitstrafe"},
        {"Sess_Issue_DriveThrough", "Durchfahrt"},
        {"Game_NetworkTransport_Connect", "Netzwerk verbinden"},
        {"GameSessionManager_OnConnectionArrived", "Verbindung angekommen"},
        {"Game_NetworkMigrateSessionHost", "Host-Migration"},
        {"Sess_StartLights_SendSequenceStartTime", "Startlichter senden"},
    };
    for (auto &paar : namen) {
        const uintptr_t adresse = swofty::api->resolve(paar[0]);
        swofty::log("[erc-liga] %s: adresse %p", paar[0], reinterpret_cast<void *>(adresse));
    }
    swofty::log("[erc-liga] Spion-Originale: Warnung=%p Zeit=%p Durchfahrt=%p", g_orig_warnung,
                g_orig_zeitstrafe, g_orig_durchfahrt);
}

// Statische Registrierung: genau der Weg, den auch die SDK-Makros nehmen.
namespace {
const swofty::detail::mixin reg_warnung{"Sess_Issue_Warning", SWOFTY_MIXIN_WRAP, 0,
                                        reinterpret_cast<void *>(&h_warnung), &g_orig_warnung};
const swofty::detail::mixin reg_zeitstrafe{"Sess_Issue_TimePenalty", SWOFTY_MIXIN_WRAP, 0,
                                           reinterpret_cast<void *>(&h_zeitstrafe), &g_orig_zeitstrafe};
const swofty::detail::mixin reg_durchfahrt{"Sess_Issue_DriveThrough", SWOFTY_MIXIN_WRAP, 0,
                                           reinterpret_cast<void *>(&h_durchfahrt), &g_orig_durchfahrt};
const swofty::detail::mixin reg_sc{"Sess_SafetyCar_ChangeState", SWOFTY_MIXIN_WRAP, 0,
                                   reinterpret_cast<void *>(&h_sc_zustand), &g_orig_sc};
const swofty::detail::mixin reg_rotflagge{"Sess_RedFlagSystem_Deploy", SWOFTY_MIXIN_WRAP, 0,
                                          reinterpret_cast<void *>(&h_rotflagge), &g_orig_rotflagge};
const swofty::detail::mixin reg_sc_logic{"Sess_SafetyCarLogic_Subscribe", SWOFTY_MIXIN_WRAP, 0,
                                         reinterpret_cast<void *>(&h_sc_logic), &g_orig_sc_logic};
const swofty::detail::mixin reg_vsc_cond{"Sess_ScCond_TriggerVsc_Update", SWOFTY_MIXIN_WRAP, 0,
                                         reinterpret_cast<void *>(&h_vsc_cond), &g_orig_vsc_cond};
const swofty::detail::mixin reg_rot_beginn{"Sess_RedFlagSystem_OnSessionBeginGameplay", SWOFTY_MIXIN_WRAP, 0,
                                           reinterpret_cast<void *>(&h_rot_beginn), &g_orig_rot_beginn};
const swofty::detail::mixin reg_net_connect{"Game_NetworkTransport_Connect", SWOFTY_MIXIN_WRAP, 0,
                                            reinterpret_cast<void *>(&h_net_connect), &g_orig_net_connect};
const swofty::detail::mixin reg_net_arrived{"GameSessionManager_OnConnectionArrived", SWOFTY_MIXIN_WRAP, 0,
                                            reinterpret_cast<void *>(&h_net_angekommen), &g_orig_net_angekommen};
const swofty::detail::mixin reg_net_migrate{"Game_NetworkMigrateSessionHost", SWOFTY_MIXIN_WRAP, 0,
                                            reinterpret_cast<void *>(&h_net_migration), &g_orig_net_migration};
const swofty::detail::mixin reg_net_lights{"Sess_StartLights_SendSequenceStartTime", SWOFTY_MIXIN_WRAP, 0,
                                           reinterpret_cast<void *>(&h_net_startlights), &g_orig_net_startlights};
} // namespace

} // namespace

SWOFTY_ON_LOAD
{
    swofty::log("[erc-liga] geladen, api v%u, befehlsdatei %s", unsigned(swofty::api->version),
                befehlsdatei().c_str());
    if (swofty::events::on_frame(bei_frame) != swofty::events::Result::ok)
        swofty::log("[erc-liga] kein Frame-Tick — Befehle kommen nicht an");
    spion_einhaengen();
    kachel_zurueckholen();
}

SWOFTY_ADDON()
