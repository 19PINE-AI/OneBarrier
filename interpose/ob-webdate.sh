#!/usr/bin/env bash
# OneBarrier — transparent deterministic recovery of unmodified web servers
# lighttpd and HAProxy (single-process / share-nothing worker, event-driven).
#   ob-webdate.sh <lighttpd|haproxy> [real_gap_seconds]
#
# Same determinism axis as the Nginx result: the HTTP Date: header is formatted from
# the server's wall clock. Under the virtual clock it is a deterministic function of
# the request sequence, so it is byte-identical across a crash + real-time gap; a
# no-libOS control's Date: tracks real time and differs.
set -u
HERE="$(cd "$(dirname "$0")" && pwd)"
OBP="$HERE/libobpreload.so"; RNG="$HERE/librngdet.so"
APP="${1:-lighttpd}"; GAP="${2:-3}"; IA='~0x4000000000000000:~0x0'
[ -f "$OBP" ] || gcc -shared -fPIC -O2 -o "$OBP" "$HERE/obpreload.c" -ldl -lpthread
[ -f "$RNG" ] || gcc -shared -fPIC -O2 -o "$RNG" "$HERE/rngdet.c" -ldl -lpthread -Wl,-Bstatic -latomic -Wl,-Bdynamic
PRE="OPENSSL_ia32cap='$IA' OB_VCLOCK=/tmp/ob-wd-$APP-vb OB_VRAND=/tmp/ob-wd-$APP-vr LD_PRELOAD='$RNG $OBP' setarch -R"
CTL="LD_PRELOAD='$OBP'"
VB=/tmp/ob-wd-$APP-vb; VR=/tmp/ob-wd-$APP-vr

case "$APP" in
  lighttpd)
    P1=8170; P2=8171; P3=8172
    mkcfg(){ cat > /tmp/ob-lt-$1.conf <<EOF
server.document-root = "/tmp"
server.port = $1
server.bind = "127.0.0.1"
server.modules = ()
EOF
}
    bcmd(){ mkcfg "$1"; echo "lighttpd -D -f /tmp/ob-lt-$1.conf"; } ;;
  haproxy)
    P1=8180; P2=8181; P3=8182
    sudo systemctl stop haproxy 2>/dev/null; sudo pkill -9 haproxy 2>/dev/null; sleep 0.5  # the apt-installed service binds and interferes
    mkcfg(){ cat > /tmp/ob-hp-$1.conf <<EOF
defaults
  mode http
  timeout connect 1s
  timeout client 1s
  timeout server 1s
frontend f
  bind 127.0.0.1:$1
  http-request return status 200 content-type text/plain hdr date "%[date(0),http_date]" string "ok\n"
EOF
}
    bcmd(){ mkcfg "$1"; echo "haproxy -f /tmp/ob-hp-$1.conf"; } ;;
  *) echo "unknown app: $APP (lighttpd|haproxy)"; exit 2 ;;
esac

kp(){ for pid in $(ss -tlnp 2>/dev/null|grep ":$1 "|grep -oP 'pid=\K[0-9]+'); do kill -9 "$pid" 2>/dev/null; done; }
up(){ for i in $(seq 50); do curl -s --max-time 1 "127.0.0.1:$1/" >/dev/null 2>&1 && return 0; sleep 0.2; done; return 1; }
drive(){ for i in $(seq 6); do curl -s -D - --max-time 2 "127.0.0.1:$1/" 2>/dev/null | grep -i '^Date:' | tr -d '\r'; done; }

L=/tmp/ob-wd-$APP-live; R=/tmp/ob-wd-$APP-replay; C=/tmp/ob-wd-$APP-ctl
kp "$P1"; kp "$P2"; kp "$P3"; sleep 0.5; rm -f "$VB" "$VR" "$L" "$R" "$C"
echo "=== $APP — deterministic Date: recovery (${GAP}s real gap) ==="
eval "$PRE $(bcmd "$P1") >/dev/null 2>&1 &"; up "$P1" && drive "$P1" > "$L"; kp "$P1"; sleep 1
echo ">>> crash; wait ${GAP}s so the real wall clock advances"; sleep "$GAP"
eval "$PRE $(bcmd "$P2") >/dev/null 2>&1 &"; up "$P2" && drive "$P2" > "$R"; kp "$P2"; sleep 1
eval "$CTL $(bcmd "$P3") >/dev/null 2>&1 &"; up "$P3" && drive "$P3" > "$C"; kp "$P3"
echo "live   : $(head -1 "$L")"
echo "replay : $(head -1 "$R")"
echo "control: $(head -1 "$C")   (real time)"
ident=no; diff -q "$L" "$R" >/dev/null 2>&1 && [ -s "$L" ] && ident=yes
ctldiff=no; [ -s "$C" ] && ! diff -q "$L" "$C" >/dev/null 2>&1 && ctldiff=yes
if [ "$ident" = yes ] && [ "$ctldiff" = yes ]; then
  echo "RESULT: $APP Date: header byte-identical across recovery, control differs ✅"; exit 0
else
  echo "RESULT: $APP check FAILED (ident=$ident ctldiff=$ctldiff)"; diff "$L" "$R" | head; exit 1
fi
