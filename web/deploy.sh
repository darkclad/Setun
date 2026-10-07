#!/bin/sh
# Deploy the browser front end to a web server: the setun-web
# container (nginx:alpine)
# serves /srv/www read-only.
#   web/deploy.sh          rebuild web/setun.wasm (web/build.sh), then copy
#   web/deploy.sh --no-build
set -e
cd "$(dirname "$0")/.."
[ "$1" = "--no-build" ] || sh web/build.sh
DEST=/srv/www
# copy under temporary names, then rename, so a reload never sees half a file
scp -q web/index.html HOST:$DEST/index.html.new
scp -q web/setun.wasm HOST:$DEST/setun.wasm.new
ssh HOST"cd $DEST && mv setun.wasm.new setun.wasm && mv index.html.new index.html && ls -l"
echo "deployed: $(git rev-parse --short HEAD)$(git diff --quiet -- crates kernel user web || echo ' (with uncommitted changes)')"
