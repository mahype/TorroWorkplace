#!/bin/sh
# Signs development builds on macOS with the identity "Torro Development" (task X.14), so the
# keychain recognises them across builds: the signature then names the certificate and a fixed
# identifier (io.github.mahype.dev.<name>) instead of a hash that changes with every build.
#
# Arguments: the programs to sign; without any, the suite's programs in target/debug. Without the
# identity (CI, other machines) it does nothing; scripts/macos-dev-cert.sh creates it.
# TORRO_DEV_KEYCHAIN names another keychain to take the identity from (used to test this script).
set -eu

NAME="Torro Development"
[ "$(uname -s)" = Darwin ] || exit 0

if [ $# -eq 0 ]; then
    root="$(cd "$(dirname "$0")/.." && pwd)"
    for name in torroworkplace torrocalendar torrotasks torroinbox torroassistant; do
        [ -x "$root/target/debug/$name" ] && set -- "$@" "$root/target/debug/$name"
    done
fi

keychain_args=""
[ -n "${TORRO_DEV_KEYCHAIN:-}" ] && keychain_args="--keychain $TORRO_DEV_KEYCHAIN"
security find-identity -v -p codesigning ${TORRO_DEV_KEYCHAIN:-} 2>/dev/null | grep -q "\"$NAME\"" || exit 0

for program in "$@"; do
    name="$(basename "$program")"
    # shellcheck disable=SC2086 # keychain_args is empty or two words on purpose
    codesign --force --sign "$NAME" $keychain_args --identifier "io.github.mahype.dev.$name" "$program" \
        2>/dev/null || echo "torro: could not sign $program with \"$NAME\"" >&2
done
