#!/bin/sh
# Copy the browser front end (the pages in web/ and web/setun.wasm) to a static
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
PAGES="index.html emu.html term.html"
for f in $PAGES setun.wasm; do scp -q "web/$f" "$DEPLOY_HOST:$DEPLOY_DIR/$f.new"; done
ssh "$DEPLOY_HOST" "cd '$DEPLOY_DIR' && mv setun.wasm.new setun.wasm && for f in $PAGES; do mv \$f.new \$f; done && ls -l"
echo "deployed: $(git rev-parse --short HEAD)$(git diff --quiet -- crates kernel user web || echo ' (with uncommitted changes)')"
