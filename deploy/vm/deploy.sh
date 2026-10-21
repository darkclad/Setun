#!/bin/sh
# Build the setun-vm image on a Docker host over ssh from the committed tree
# (git archive HEAD) and restart the container; its disk volume is kept.
# Where to: VM_HOST (an ssh destination) and VM_DIR (a directory there for
# the build context and the /data volume), from the environment or from
# web/deploy.env. VM_NETWORK (optional): a Docker network to join, so a web
# server on it reaches ttyd as setun-vm:7681.
set -e
cd "$(dirname "$0")/../.."
[ -f web/deploy.env ] && . web/deploy.env
: "${VM_HOST:?set VM_HOST (or add it to web/deploy.env)}"
: "${VM_DIR:?set VM_DIR (or add it to web/deploy.env)}"
NET=${VM_NETWORK:+--network $VM_NETWORK}
ssh "$VM_HOST" "rm -rf '$VM_DIR/build' && mkdir -p '$VM_DIR/build' '$VM_DIR/data'"
git archive --format=tar HEAD | ssh "$VM_HOST" "tar -x -C '$VM_DIR/build'"
ssh "$VM_HOST" "cd '$VM_DIR/build' && docker build -q -f deploy/vm/Dockerfile -t setun-vm:latest . \
  && (docker rm -f setun-vm >/dev/null 2>&1 || true) \
  && docker run -d --name setun-vm --restart unless-stopped $NET -p 2227:2227 -v '$VM_DIR/data:/data' setun-vm:latest"
echo "setun-vm: $(git rev-parse --short HEAD)"
