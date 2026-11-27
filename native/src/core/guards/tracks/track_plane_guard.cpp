#include "core/guards/guard.hpp"
#include "core/guards/lines.hpp"
#include "core/runtime/loader/hook_install.hpp"
#include "core/runtime/runtime.hpp"

#include <windows.h>

#include <safetyhook.hpp>
#include <swofty/mem.hpp>
#include <swofty/mixin.hpp>

#include <atomic>
#include <cstdint>

// Track volume plane set (no KB function name; F1_25.exe+0x3960740 on the Steam build the field runs, PE stamp
// 1794312636).
//
// The function takes a convex volume's plane set in rcx: it negates six plane pairs from [rcx]..[rcx+0x50], then walks
// the bit words in r9 (64 points per word, rdx = the points, 0x10 bytes each, the fifth argument on the stack = the
// output buffer it fills with a dword per point inside) and returns the number of dwords it wrote. rcx comes from a
// pointer table of the caller (the node at r15: count +0x120, table +0x128), and nothing checks it: a null entry faults
// ACCESS_VIOLATION reading 0x0 at +0x5a, on every worker thread at once.
//
// A 20.8 km track built on the Portimão carrier with the map-editor toolbox faults there about 95 s after the load
// finishes, with the ring and with a plain oval (only the centreline differs), at every TS volume spacing tried
// (695 and 232 volumes; the Nürburgring GP addon built the same way, 171 volumes, loads). The set that is null is not
// identified yet: the guard skips that volume's point test (the caller reads it as "no point of this volume is inside"),
// which leaves the points out of the culling result, and logs the first eight skips with the count, the point words and
// the output buffer. The line names the volume next time the track loads.
//
// The site is found by byte scan: the 24-byte prologue matches once in the exe (base+0x3960740, in .tls$, this build's
// code section), so the guard needs no binding and scans every executable section.
//
// SWOFTY_TRACK_PLANE_GUARD=0 turns it off.

namespace {

namespace guards = swofty_core::guards;

// The shipped Steam build keeps its code in .tls$; the KB build and the EA dumps use .xtls. Both are tried.
const runtime::Sig S_PLANE_SET_TLS{"track volume plane set filter (prologue, no KB name)",
                                   "41 56 56 57 55 53 48 81 EC A0 00 00 00 C5 78 29 BC 24 90 00 00 00",
                                   0x3960740,
                                   runtime::Sig::addr,
                                   0,
                                   0,
                                   0,
                                   ".tls$"};
const runtime::Sig S_PLANE_SET_XTLS{"track volume plane set filter (prologue, no KB name)",
                                    "41 56 56 57 55 53 48 81 EC A0 00 00 00 C5 78 29 BC 24 90 00 00 00",
                                    0x3960740,
                                    runtime::Sig::addr,
                                    0,
                                    0,
                                    0,
                                    ".xtls"};

// The call site (base+0x3df302b) hands the plane set in rcx from the caller's local +0x80a8 and keeps the object it
// works on at +0x8230 (its record at +0x8210, the list cursor at +0x8098, the list holder at +0x8148). Logging the
// object when its plane set is null names the structure that carries the missing set.
const runtime::Sig S_CALL_SITE_TLS{"track volume plane set call site (mov rcx,[rbp+0x80a8]; call)",
                                   "48 8B 8D A8 80 00 00 41 89 D8 4C 8D 8D 80 7C 00 00 E8",
                                   0x3df302b,
                                   runtime::Sig::addr,
                                   0,
                                   0,
                                   0,
                                   ".tls$"};
const runtime::Sig S_CALL_SITE_XTLS{"track volume plane set call site (mov rcx,[rbp+0x80a8]; call)",
                                    "48 8B 8D A8 80 00 00 41 89 D8 4C 8D 8D 80 7C 00 00 E8",
                                    0x3df302b,
                                    runtime::Sig::addr,
                                    0,
                                    0,
                                    0,
                                    ".xtls"};

// The caller's null-record path and its loop head (both match once, base+0x3df31b0 and base+0x3df2a80).
const runtime::Sig S_NULL_RECORD_TLS{"track volume record read (mov rax,[rbp+0x8210])",
                                     "48 8B 85 10 82 00 00 44 8B 68 3C",
                                     0x3df31b0,
                                     runtime::Sig::addr,
                                     0,
                                     0,
                                     0,
                                     ".tls$"};
const runtime::Sig S_LOOP_TLS{"track volume list loop head", "C5 C0 57 FF C5 7A 10 05 94 17 59 01 4C 8B AD B0 81 00 00",
                              0x3df2a80,
                              runtime::Sig::addr,
                              0,
                              0,
                              0,
                              ".tls$"};

constexpr int LOGGED_SKIPS = 8;
constexpr uintptr_t L_OBJECT = 0x8230, L_RECORD = 0x8210, L_PLANES = 0x80a8, L_CURSOR = 0x8098, L_HOLDER = 0x8148;

using PlaneSet_t = uintptr_t(void *planes, const void *points, uint32_t count, const void *words, void *out);

safetyhook::InlineHook h_plane_set;
safetyhook::MidHook h_call_site;
std::atomic<int> skipped{0};
std::atomic<int> described{0};
std::atomic<int> skipped_record{0};
uintptr_t loop_at = 0;

// A pointer the game can read: committed, not a guard page, and 0x60 bytes inside one region. The plane set the
// caller passes is not always a pointer at all (0x0, 0x1fffffffffff and 0x7fffffffff were seen), so a plain null test
// is not enough.
bool lesbar(uintptr_t at, size_t n)
{
    if (at < 0x1000 || at > 0x7ffffffeffffull)
        return false;
    MEMORY_BASIC_INFORMATION m;
    if (!VirtualQuery(reinterpret_cast<const void *>(at), &m, sizeof m))
        return false;
    return m.State == MEM_COMMIT && !(m.Protect & (PAGE_NOACCESS | PAGE_GUARD)) &&
           uintptr_t(m.BaseAddress) + m.RegionSize >= at + n;
}

// Raw read of the caller's frame: these offsets are inside the running thread's stack, which swofty::mem::read
// refuses (it validates heap objects), so it would answer 0 for every field. A read of an address that is not
// mapped returns 0 instead of faulting in the detour.
uintptr_t lesen(uintptr_t at)
{
    return lesbar(at, sizeof(uintptr_t)) ? *reinterpret_cast<const uintptr_t *>(at) : 0;
}

uintptr_t plane_set(void *planes, const void *points, uint32_t count, const void *words, void *out)
{
    const uintptr_t p = reinterpret_cast<uintptr_t>(planes);
    if (!lesbar(p, 0x60)) {
        if (skipped.fetch_add(1) < 24)
            swofty::log("[planes] a track volume has no readable plane set (%p, %u point(s), from base+0x%llx): its points "
                        "are left out",
                        planes, count,
                        (unsigned long long)((uintptr_t)__builtin_return_address(0) -
                                             (uintptr_t)GetModuleHandleW(L"F1_25.exe")));
        return 0;
    }
    return h_plane_set.call<uintptr_t>(planes, points, count, words, out);
}

// The caller's "no point inside" path reads the object's record at +0x8210 and faults when it is null. That object is
// the same one whose plane set was missing; the loop continues with the next entry (base+0x3df2a80) instead.
void null_record(safetyhook::Context &c)
{
    uintptr_t record = c.rbp ? lesen(c.rbp + L_RECORD) : 0;
    if (lesbar(record, 0x48))
        return;
    if (loop_at)
        c.rip = loop_at;
    if (skipped_record.fetch_add(1) < LOGGED_SKIPS)
        swofty::log("[planes] an object without a record (object %p record %p): its entry is skipped",
                    c.rbp ? lesen(c.rbp + L_OBJECT) : 0, record);
}

void setup()
{
    char v[16] = {};
    if (guards::env_off("SWOFTY_TRACK_PLANE_GUARD", v)) {
        swofty::log("%s", guards::lines::off("[planes] track plane guard", "SWOFTY_TRACK_PLANE_GUARD", v).c_str());
        return;
    }
    uintptr_t at = runtime::find(S_PLANE_SET_TLS, "plane", "planes");
    if (!at)
        at = runtime::find(S_PLANE_SET_XTLS, "plane", "planes");
    if (!at) {
        swofty::log("[planes] track plane guard off: the volume plane set filter did not resolve on this build");
        return;
    }
    runtime::loader::Installed r =
        runtime::loader::install(h_plane_set, at, S_PLANE_SET_TLS.what, reinterpret_cast<void *>(&plane_set));
    if (!r) {
        swofty::log("%s", guards::lines::hook_failed("[planes] track plane guard", S_PLANE_SET_TLS.what, r.error).c_str());
        return;
    }
    loop_at = runtime::find(S_LOOP_TLS, "plane", "planes");
    uintptr_t record_at = runtime::find(S_NULL_RECORD_TLS, "plane", "planes");
    if (record_at && loop_at) {
        runtime::loader::Installed m =
            runtime::loader::install(h_call_site, record_at, S_NULL_RECORD_TLS.what, null_record);
        if (!m)
            swofty::log("%s", guards::lines::hook_failed("[planes] track plane guard", S_NULL_RECORD_TLS.what, m.error).c_str());
    } else {
        swofty::log("[planes] track plane guard: the caller's record read did not resolve, so an object without a "
                    "record still faults");
    }
    swofty::log("%s", guards::lines::armed("[planes] track plane guard",
                                           "a track volume whose plane set or record is not readable is left out of the "
                                           "culling result instead of faulting the load")
                          .c_str());
}

}

SWOFTY_HOOK_GUARD(track_plane_set, "track plane guard", setup)
