#!/bin/sh
# The container's processes: the machine in a tmux session (its serial
# console needs a terminal; `docker exec -it setun-vm tmux attach -t vm`
# shows it, Ctrl-b d leaves), and ttyd in front.
set -e
DISK=/data/trinux.disk
[ -f "$DISK" ] || mkdisk -o "$DISK"
# Booted again whenever it stops (poweroff, reboot, a crash). The forward
# listens on every address of the container, so the published port reaches it.
tmux new-session -d -s vm -x 100 -y 30 \
  "while :; do setun --console --disk $DISK --net 0.0.0.0:2227:22; echo 'setun stopped; booting again'; sleep 3; done"
# One login per browser connection, at most 27 at once.
exec ttyd --port 7681 --base-path /term --writable --max-clients 27 \
  --client-option titleFixed=Setun-11 \
  --client-option 'theme={"background":"#000000","foreground":"#dadfcd","cursor":"#f2a73b"}' \
  login.sh
