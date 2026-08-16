#!/usr/bin/env bash
#
# Sign MinnowSnap.app with a STABLE, self-signed code-signing identity so that
# macOS remembers the Screen Recording (TCC) grant across rebuilds.
#
# Why this exists
# ---------------
# `codesign --sign -` produces an *ad-hoc* signature with no stable identity.
# macOS TCC keys the Screen Recording permission on the app's code-signature
# identity; for an ad-hoc signature the designated requirement is the cdhash,
# which changes on every rebuild. So each re-signed build looks like a brand
# new app and TCC re-prompts (and never remembers the grant).
#
# Signing with a persistent self-signed certificate instead gives the bundle a
# stable designated requirement:
#
#     identifier "com.lortunate.minnowsnap" and certificate leaf = H"<cert>"
#
# That depends only on the bundle id and the certificate — both stable across
# rebuilds — not on the cdhash. TCC then recognizes the same app after every
# re-sign and the grant sticks: authorize once, never again.
#
# The certificate lives in your login keychain and is created once on first
# run. macOS may show a one-time keychain prompt the first time codesign uses
# the key ("codesign wants to sign using key ...") — click *Always Allow*. That
# is a keychain prompt, not the Screen Recording prompt, and it does not recur.
#
# Usage:
#   scripts/macos-sign.sh [path/to/MinnowSnap.app]
#
# Defaults to target/release/bundle/osx/MinnowSnap.app.

set -euo pipefail

IDENTITY_CN="MinnowSnap Local Signing"
KEYCHAIN="${HOME}/Library/Keychains/login.keychain-db"
# Transient password for the intermediate PKCS#12 bundle. It only guards the
# few-millisecond hop from openssl to `security import`; the key at rest is
# protected by the login keychain, not this string.
P12_PASS="minnowsnap-local"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
APP_PATH="${1:-${PROJECT_ROOT}/target/release/bundle/osx/MinnowSnap.app}"

log() { printf '%12s %s\n' "$1" "$2"; }

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "macos-sign.sh only runs on macOS" >&2
    exit 1
fi

if [[ ! -d "${APP_PATH}" ]]; then
    echo "Error: bundle not found at ${APP_PATH}" >&2
    echo "Build it first: scripts/bundle.py, or" >&2
    echo "  (cd crates/minnow-app && cargo bundle --release --format osx)" >&2
    exit 1
fi

# The self-signed cert never chains to a trusted root, so it does not appear in
# `security find-identity -p codesigning`. Detect it by certificate presence in
# the keychain instead; codesign can still sign with it by common name.
identity_exists() {
    security find-certificate -c "${IDENTITY_CN}" "${KEYCHAIN}" >/dev/null 2>&1
}

ensure_identity() {
    if identity_exists; then
        log "Signing" "reusing identity \"${IDENTITY_CN}\""
        return
    fi

    log "Signing" "creating self-signed identity \"${IDENTITY_CN}\""
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' RETURN

    # Self-signed leaf certificate marked for code signing.
    openssl req -x509 -newkey rsa:2048 -sha256 -days 3650 -nodes \
        -keyout "${tmp}/key.pem" -out "${tmp}/cert.pem" \
        -subj "/CN=${IDENTITY_CN}" \
        -addext "basicConstraints=critical,CA:FALSE" \
        -addext "keyUsage=critical,digitalSignature" \
        -addext "extendedKeyUsage=critical,codeSigning" \
        >/dev/null 2>&1

    # Apple's SecKeychain PKCS#12 importer rejects OpenSSL 3.x defaults (it
    # reports a bogus "wrong password" MAC failure). Force the legacy 3DES/SHA1
    # algorithms and a non-empty password, which the importer accepts.
    openssl pkcs12 -export -out "${tmp}/identity.p12" \
        -inkey "${tmp}/key.pem" -in "${tmp}/cert.pem" \
        -passout "pass:${P12_PASS}" \
        -legacy -keypbe PBE-SHA1-3DES -certpbe PBE-SHA1-3DES -macalg sha1 \
        >/dev/null 2>&1

    # Import key + cert and pre-authorize codesign to use the key.
    security import "${tmp}/identity.p12" -k "${KEYCHAIN}" -P "${P12_PASS}" \
        -T /usr/bin/codesign >/dev/null 2>&1

    # Let apple tools (codesign) use the key without a per-run keychain prompt.
    # Harmless if it fails on a locked/non-default keychain.
    security set-key-partition-list -S apple-tool:,apple: -s \
        -k "" "${KEYCHAIN}" >/dev/null 2>&1 || true

    if ! identity_exists; then
        echo "Error: failed to create signing identity \"${IDENTITY_CN}\"" >&2
        exit 1
    fi
}

ensure_identity

log "Signing" "${APP_PATH}"
codesign --force --deep --sign "${IDENTITY_CN}" "${APP_PATH}"

log "Verify" "signature"
codesign -dvvv "${APP_PATH}" 2>&1 | grep -E "Identifier=|Authority=|Signature=" || true
codesign --verify --deep --strict "${APP_PATH}" && log "Verify" "OK (valid on-disk signature)"

echo
log "Requirement" "designated requirement (stable across rebuilds):"
codesign -d -r- "${APP_PATH}" 2>&1 | grep "designated" || true

cat <<EOF

Done. The bundle is signed with a stable identity, so its designated
requirement depends only on the bundle id and this certificate — not on the
per-build cdhash. Grant Screen Recording once (System Settings > Privacy &
Security > Screen & System Audio Recording, or when first prompted); later
re-signs with this script keep the grant instead of re-prompting.
EOF
