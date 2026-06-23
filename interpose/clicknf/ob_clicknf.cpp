// SPDX-License-Identifier: Apache-2.0
// ob_clicknf — a software Click network function (host CPU) over the OpenClickNP
// runtime, made transparently crash-recoverable by the OneBarrier libOS.
//
// The NF is a stateful L4 load balancer with connection tracking, built from the
// OpenClickNP FlowCache element (direct-mapped learning flow table) + the
// L4LoadBalancer element (hash a new flow onto a backend). Per flow it keeps:
//   - the assigned backend (affinity: a flow keeps its backend across packets), and
//   - a conntrack `last_seen` timestamp read from the clock.
// Packets arrive over a socket, so under the OneBarrier libOS the virtual clock
// advances one tick per packet: the last_seen timestamps become a deterministic
// function of the packet sequence. Replaying the (fabric-ordered) packet log after
// a crash therefore rebuilds the EXACT flow table — backend affinity AND conntrack
// timers — which a stateless NF restart cannot do (the FTMB / Pico-Replication
// stateful-middlebox-FT problem). This is the OpenClickNP NF lineage (SIGCOMM'16),
// run in software and made FT as a byproduct of ordered replay.
//
// Line protocol (one packet per line):
//   P <flowkey>   -> process a packet for flow <flowkey>; reply: <HIT|MISS> k b ts
//   DUMP          -> dump the whole flow table (idx|key|backend|last_seen), sorted
//   QUIT
#include "openclicknp/flit.hpp"
#include <cstdio>
#include <cstdint>
#include <cstring>
#include <ctime>
#include <string>
#include <vector>
#include <algorithm>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <unistd.h>

using openclicknp::flit_t;

// ---- FlowCache element state (elements/lookups/FlowCache.clnp), 1024-entry
//      direct-mapped learning table, extended with backend affinity + conntrack. ----
static const uint32_t SIZE = 1024;
struct Table {
    uint32_t key[SIZE];   bool valid[SIZE];
    uint32_t backend[SIZE];           // L4LoadBalancer assignment (affinity)
    long long last_ns[SIZE];          // conntrack last-seen (virtual time)
} T;
static const uint32_t NBACKENDS = 8;

static long long now_ns() {            // intercepted by the libOS virtual clock
    struct timespec ts; clock_gettime(CLOCK_REALTIME, &ts);
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

// Process one packet (flow key) through the NF element graph. Returns the reply.
static std::string process(uint32_t k) {
    // Build a flit the way the OpenClickNP topology would (flow key in lane 0).
    flit_t f{}; f.set(0, k); f.set_sop(true); f.set_eop(true);
    uint32_t k32 = static_cast<uint32_t>(f.get(0));
    uint32_t i = k32 & (SIZE - 1);     // FlowCache direct-mapped index
    long long ts = now_ns();
    bool hit = T.valid[i] && T.key[i] == k32;
    if (!hit) {                        // miss: learn the flow, assign a backend
        T.key[i] = k32; T.valid[i] = true;
        T.backend[i] = (uint32_t)(((uint64_t)k32 * 2654435761ULL) % NBACKENDS); // L4LB hash
    }
    T.last_ns[i] = ts;                 // conntrack: stamp last-seen (virtual time)
    f.set(2, T.backend[i]);            // L4LoadBalancer writes backend into lane 2
    char buf[96];
    std::snprintf(buf, sizeof buf, "%s %u %u %lld\n", hit ? "HIT" : "MISS", k32, T.backend[i], ts);
    return buf;
}

static std::string dump() {
    std::vector<uint32_t> idx;
    for (uint32_t i = 0; i < SIZE; ++i) if (T.valid[i]) idx.push_back(i);
    std::sort(idx.begin(), idx.end());
    std::string out;
    char buf[128];
    for (uint32_t i : idx) {
        std::snprintf(buf, sizeof buf, "%u|%u|%u|%lld\n", i, T.key[i], T.backend[i], T.last_ns[i]);
        out += buf;
    }
    out += "END\n";
    return out;
}

static void serve(int cfd) {
    std::string in;
    char rb[4096];
    for (;;) {
        ssize_t n = read(cfd, rb, sizeof rb);
        if (n <= 0) return;
        in.append(rb, n);
        size_t nl;
        while ((nl = in.find('\n')) != std::string::npos) {
            std::string line = in.substr(0, nl); in.erase(0, nl + 1);
            if (!line.empty() && line.back() == '\r') line.pop_back();
            std::string reply;
            if (line.rfind("P ", 0) == 0) reply = process((uint32_t)strtoul(line.c_str() + 2, nullptr, 10));
            else if (line == "DUMP")      reply = dump();
            else if (line == "QUIT")      return;
            else continue;
            (void)!write(cfd, reply.data(), reply.size());
        }
    }
}

int main(int argc, char** argv) {
    int port = argc > 1 ? atoi(argv[1]) : 9300;
    std::memset(&T, 0, sizeof T);
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    int one = 1; setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);
    sockaddr_in a{}; a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); a.sin_port = htons(port);
    if (bind(fd, (sockaddr*)&a, sizeof a) < 0) { perror("bind"); return 1; }
    listen(fd, 16);
    std::fprintf(stderr, "ob_clicknf (OpenClickNP FlowCache+L4LB) listening on 127.0.0.1:%d\n", port);
    for (;;) { int c = accept(fd, nullptr, nullptr); if (c < 0) continue; serve(c); close(c); }
    return 0;
}
