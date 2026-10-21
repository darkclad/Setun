# A Setun-11̅ running Trinux all the time, for logging in over SSH:
#   port 2227   Trinux's sshd (a real ssh client: ssh -p 2227 user@HOST)
#   port 7681   ttyd: a terminal in the browser that logs in over that ssh
#               (served under /term/, for web/term.html)
# The disk is /data/trinux.disk, made on first start; keep /data on a volume.
#
#   docker build -f deploy/vm/Dockerfile -t setun-vm .
#   docker run -d --name setun-vm -p 2227:2227 -p 7681:7681 -v setun-data:/data setun-vm

FROM rust:1-trixie AS build
WORKDIR /src
COPY . .
RUN cargo build --release -p setun-native -p trinux --bin setun --bin mkdisk

FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends openssh-client tmux tini ca-certificates \
 && rm -rf /var/lib/apt/lists/*
# ttyd is not packaged by Debian: its own static release, pinned by hash
ADD --checksum=sha256:8a217c968aba172e0dbf3f34447218dc015bc4d5e59bf51db2f2cd12b7be4f55 \
    https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.x86_64 /usr/local/bin/ttyd
RUN chmod +x /usr/local/bin/ttyd
# mkdisk reads the sources from where they were built (/src)
COPY --from=build /src/kernel /src/kernel
COPY --from=build /src/user /src/user
COPY --from=build /src/fonts /src/fonts
COPY --from=build /src/crates/trine/lib /src/crates/trine/lib
# the paths are crates/trinux/../../kernel and so on: the directory must exist
RUN mkdir -p /src/crates/trinux
COPY --from=build /src/target/release/setun /src/target/release/mkdisk /usr/local/bin/
COPY deploy/vm/start.sh deploy/vm/login.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/start.sh /usr/local/bin/login.sh
VOLUME /data
EXPOSE 2227 7681
ENTRYPOINT ["tini", "--", "start.sh"]
