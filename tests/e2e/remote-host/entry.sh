#!/bin/sh
# Install the test public key, start sshd, then hand over to the stock dind entrypoint.
set -e
mkdir -p /root/.ssh && chmod 700 /root/.ssh
if [ -n "$LTN_E2E_PUBKEY" ]; then echo "$LTN_E2E_PUBKEY" > /root/.ssh/authorized_keys; fi
chmod 600 /root/.ssh/authorized_keys
/usr/sbin/sshd
exec dockerd-entrypoint.sh "$@"
