#!/bin/sh
# Copy the browser front end (web/index.html and web/setun.wasm) to a static
# web server over ssh. Where to: DEPLOY_HOST (an ssh destination) and
# DEPLOY_DIR (the directory the server serves), from the environment or from
# web/deploy.env (not tracked; two lines of KEY=value).
#   web/deploy.sh          rebuild web/setun.wasm (web/build.sh), then copy
#   web/deploy.sh --no-build
set -e
cd "$(dirname "$0")/.."
[ -f web/deploy.env ] && . web/deploy.env
: "${DEPLOY_HOST:?set DEPLOY_HOST (or write web/deploy.env)}"
: "${DEPLOY_DIR:?set DEPLOY_DIR (or write web/deploy.env)}"
[ "$1" = "--no-build" ] || sh web/build.sh
# copy under temporary names, then rename, so a reload never sees half a file
scp -q web/index.html "$DEPLOY_HOST:$DEPLOY_DIR/index.html.new"
scp -q web/setun.wasm "$DEPLOY_HOST:$DEPLOY_DIR/setun.wasm.new"
ssh "$DEPLOY_HOST" "cd '$DEPLOY_DIR' && mv setun.wasm.new setun.wasm && mv index.html.new index.html && ls -l"
echo "deployed: $(git rev-parse --short HEAD)$(git diff --quiet -- crates kernel user web || echo ' (with uncommitted changes)')"
