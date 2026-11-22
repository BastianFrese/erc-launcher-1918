// Host-native unit test for the room set-up's teardown guard (native/src/core/netplay/netplay_room_logic.hpp).
// Built and run by native/test/run_tests.sh netplay_room.

#include "core/netplay/netplay_room_logic.hpp"

#include <cstdio>
#include <map>

namespace nr = runtime::netplay_room;

static int failed = 0;

static void expect(bool ok, const char *what)
{
    std::printf("%s: %s\n", ok ? "ok  " : "FAIL", what);
    if (!ok)
        failed++;
}

namespace {

// A fake image: only the bytes a test writes exist, everything else reads as unreadable.
struct Image {
    std::map<uint64_t, uint8_t> bytes;

    void q(uint64_t at, uint64_t v)
    {
        for (size_t i = 0; i < 8; i++)
            bytes[at + i] = uint8_t(v >> (8 * i));
    }
    void clear(uint64_t at)
    {
        for (size_t i = 0; i < 8; i++)
            bytes.erase(at + i);
    }
};

bool read_at(uint64_t address, void *out, size_t n, void *ctx)
{
    Image &img = *static_cast<Image *>(ctx);
    auto *p = static_cast<uint8_t *>(out);
    for (size_t i = 0; i < n; i++) {
        auto it = img.bytes.find(address + i);
        if (it == img.bytes.end())
            return false;
        p[i] = it->second;
    }
    return true;
}

nr::Memory memory_of(Image &img) { return {read_at, &img}; }

constexpr uint64_t NS = 0x100000, GSUI = 0x200000, INVITES = 0x300000;
constexpr uint64_t START = 1000, TAKEN = 1300;

// The run up to the point where the join request has been taken: mode switch, the switch's check, the request
// write and the tick that sees the game take it. Returns the tick at which it was taken.
void reach_interrupt(nr::Run &r, Image &img, nr::Memory m)
{
    r = {};
    r.start(nr::Role::join, START, true, false);
    nr::Facts f;
    f.home = false;
    f.lan_mode = true;
    expect(r.tick(f, START) == nr::Do::mode_switch, "the first tick asks for the mode switch");
    expect(r.tick(f, START + 100) == nr::Do::nothing, "the switched mode is seen, no call yet");
    expect(r.tick(f, START + 200) == nr::Do::join_write, "then the join request is written");
    f.pending_taken = true;
    expect(r.tick(f, TAKEN) == nr::Do::nothing, "the game taking the request is not a call");
    (void)img;
    (void)m;
}

} // namespace

int main()
{
    {
        Image img;
        img.q(GSUI + nr::GSUI_NS, NS);
        nr::Memory m = memory_of(img);
        expect(nr::still_attached(m, GSUI, NS), "a GameSessionUI that points back at the network subsystem is alive");
        expect(!nr::still_attached(m, GSUI, NS + 1), "one that points at another subsystem is not");
        expect(!nr::still_attached(m, 0, NS) && !nr::still_attached(m, GSUI, 0), "no object or no subsystem is not alive");
        img.clear(GSUI + nr::GSUI_NS);
        expect(!nr::still_attached(m, GSUI, NS), "a cleared back pointer (the game tore the session down) is not alive");
    }
    {
        // Zorax's case: the game tore the session down after it took the request, before the lobby.
        nr::Run r;
        Image img;
        nr::Memory m = memory_of(img);
        reach_interrupt(r, img, m);
        nr::Facts f;
        f.home = false;
        f.lan_mode = true;
        f.pending_taken = true;
        f.session_alive = false;
        expect(r.tick(f, TAKEN + 100) == nr::Do::nothing, "a torn-down session asks for no call");
        expect(r.failed(), "the run ends instead of waiting for the lobby");
        expect(r.error.find("tore the session down") != std::string::npos, "and says that the game tore it down");
    }
    {
        // The by-hand join is not requested when the session went away at the moment it would be due.
        nr::Run r;
        Image img;
        nr::Memory m = memory_of(img);
        reach_interrupt(r, img, m);
        nr::Facts f;
        f.home = false;
        f.lan_mode = true;
        f.pending_taken = true;
        f.invite.readable = true;
        f.invite.type = nr::invite_type_for(nr::Role::join);
        f.session_alive = true;
        uint64_t due = TAKEN + nr::INTERRUPT_MS;
        expect(r.tick(f, due - 1) == nr::Do::nothing, "the by-hand join waits for its deadline");
        f.session_alive = false;
        expect(r.tick(f, due) == nr::Do::nothing, "and is dropped when the session was torn down meanwhile");
        expect(r.failed(), "which fails the run");
    }
    {
        // The path that still works: the invite is there, no delegate, the by-hand request goes out.
        nr::Run r;
        Image img;
        nr::Memory m = memory_of(img);
        reach_interrupt(r, img, m);
        nr::Facts f;
        f.home = false;
        f.lan_mode = true;
        f.pending_taken = true;
        f.invite.readable = true;
        f.invite.type = nr::invite_type_for(nr::Role::join);
        expect(r.tick(f, TAKEN + nr::INTERRUPT_MS + 1) == nr::Do::fallback_notify,
               "an invite without a delegate still gets the by-hand join");
        expect(r.by_hand, "and the run marks the join as made by hand");
    }
    {
        // An invite of the wrong type is still a named failure, not a call into the game.
        nr::Run r;
        Image img;
        nr::Memory m = memory_of(img);
        reach_interrupt(r, img, m);
        nr::Facts f;
        f.home = false;
        f.lan_mode = true;
        f.pending_taken = true;
        f.invite.readable = true;
        f.invite.type = nr::INVITE_CREATE_CUSTOM;
        expect(r.tick(f, TAKEN + nr::INTERRUPT_MS + 1) == nr::Do::nothing, "a foreign invite asks for no call");
        expect(r.failed() && r.error.find("never reached the invite manager") != std::string::npos,
               "and fails with the invite that never arrived");
    }

    std::printf("%s: %d check(s) failed\n", failed ? "FAIL" : "ok  ", failed);
    return failed ? 1 : 0;
}
