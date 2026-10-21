#!/bin/sh
# What ttyd runs for each browser connection: ask for an account name, then
# log in to Trinux's sshd over the forward (Trinux asks for the password).
# The machine's host key is remembered on the volume.
printf 'Setun-11\314\205 \342\200\224 Trinux on the server\r\n\r\n'
while :; do
  printf 'login: '
  read -r name || exit 0
  case "$name" in
    '') continue ;;
    *[!a-z0-9_-]*|-*) printf 'an account name: lower-case letters, digits, _ and -\r\n'; continue ;;
  esac
  break
done
exec ssh -p 2227 -o HostName=127.0.0.1 -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile=/data/known_hosts \
  -o ServerAliveInterval=30 -o LogLevel=ERROR "$name@setun"
