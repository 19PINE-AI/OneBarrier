# OneBarrier libOS — shared shell helpers.
#
# Sourced by every harness in this directory. Its one job is to guarantee that
# the LD_PRELOAD shims exist before a script tries to preload them.
#
# This matters more than it looks: the dynamic loader IGNORES a missing library
# named in LD_PRELOAD, silently and without an error. A harness that preloads a
# shim that was never compiled therefore runs the app with NO interposition at
# all and reports a determinism FAILURE — a real result that is really wrong.
# `ob_require_shims` turns that silent misfire into either a build or a loud
# error.

ob_shim_src() {
  case "$1" in
    libobpreload.so) echo obpreload.c ;;
    librngdet.so)    echo rngdet.c ;;
    libdetsched.so)  echo detsched.c ;;
    *) return 1 ;;
  esac
}

# ob_require_shims <name.so> [name.so ...]
# Builds any shim that is missing or older than its source. Exits non-zero with
# a diagnostic if it cannot be built, rather than letting LD_PRELOAD no-op.
ob_require_shims() {
  local dir src lib
  dir="${OB_INTERPOSE_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"
  for lib in "$@"; do
    src="$(ob_shim_src "$lib")" || { echo "ob: unknown shim '$lib'" >&2; return 2; }
    if [ -f "$dir/$lib" ] && [ "$dir/$lib" -nt "$dir/$src" ]; then
      continue
    fi
    if ! command -v gcc >/dev/null 2>&1; then
      echo "ob: $lib is missing and gcc is not installed." >&2
      echo "ob: install a C compiler (Debian/Ubuntu: sudo apt-get install build-essential)" >&2
      echo "ob: or build the shims yourself: make -C '$dir'" >&2
      return 1
    fi
    echo "ob: building $lib from $src" >&2
    if ! gcc -shared -fPIC -O2 -o "$dir/$lib" "$dir/$src" -ldl -lpthread; then
      echo "ob: failed to build $lib from $src" >&2
      return 1
    fi
  done
  return 0
}
