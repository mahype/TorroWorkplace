#!/bin/sh
# Creates the code-signing identity "Torro Development" for development builds on macOS (task X.14
# in docs/tasks/plattformen.md): a self-signed certificate with its key in the login keychain.
#
# Run it once yourself. macOS asks for your password: codesign only signs with a certificate that is
# trusted for code signing, and the script marks this one so (for you only, not system-wide).
# Afterwards `cargo run` signs what it starts with this identity (scripts/macos-dev-run.sh), so
# "Always Allow" in a keychain prompt outlasts the next build. An optional argument names another
# keychain; then the certificate is only imported, not trusted (used to test this script).
set -eu

NAME="Torro Development"
KEYCHAIN="${1:-$HOME/Library/Keychains/login.keychain-db}"

if security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null | grep -q "\"$NAME\""; then
    echo "\"$NAME\" is already in $KEYCHAIN and ready to sign."
    exit 0
fi
if security find-certificate -c "$NAME" "$KEYCHAIN" >/dev/null 2>&1; then
    echo "\"$NAME\" is in $KEYCHAIN but not trusted for code signing; remove it in Keychain Access and run this again." >&2
    exit 1
fi

dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
cat >"$dir/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $NAME
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CNF

# macOS's own openssl (LibreSSL) writes a PKCS#12 file the keychain can import.
/usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$dir/cert.cnf" \
    -keyout "$dir/key.pem" -out "$dir/cert.pem" 2>/dev/null
/usr/bin/openssl pkcs12 -export -inkey "$dir/key.pem" -in "$dir/cert.pem" -name "$NAME" \
    -out "$dir/identity.p12" -passout pass:torro
security import "$dir/identity.p12" -k "$KEYCHAIN" -P torro -T /usr/bin/codesign >/dev/null
if [ $# -eq 0 ]; then
    security add-trusted-cert -r trustRoot -p codeSign -k "$KEYCHAIN" "$dir/cert.pem"
fi

echo "\"$NAME\" is now in $KEYCHAIN. \`cargo run\` signs development builds with it."
