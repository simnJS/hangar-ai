# Test node for hangar-node: a bare Debian with sshd (key auth only) and the
# tools a remote node needs. Pinned by dated tag AND digest.
FROM debian:trixie-20251020-slim@sha256:66b37a5078a77098bfc80175fb5eb881a3196809242fd295b25502854e12cbec

# Off by default: no Claude login happens in tests.
ARG WITH_CLAUDE=0

RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      openssh-server git bash curl ca-certificates procps iproute2 \
 && rm -rf /var/lib/apt/lists/*

RUN if [ "$WITH_CLAUDE" = "1" ]; then \
      apt-get update \
      && apt-get install -y --no-install-recommends nodejs npm \
      && npm install -g @anthropic-ai/claude-code \
      && rm -rf /var/lib/apt/lists/*; \
    fi

RUN useradd --create-home --shell /bin/bash hangar \
 && usermod -p '*' hangar \
 && mkdir -p /run/sshd /home/hangar/.ssh \
 && chmod 700 /home/hangar/.ssh \
 && chown -R hangar:hangar /home/hangar/.ssh \
 && ssh-keygen -A

RUN printf '%s\n' \
      'Port 22' \
      'PermitRootLogin no' \
      'PasswordAuthentication no' \
      'KbdInteractiveAuthentication no' \
      'PubkeyAuthentication yes' \
      'AuthenticationMethods publickey' \
      'AllowUsers hangar' \
      'UsePAM no' \
      > /etc/ssh/sshd_config.d/10-hangar-test.conf

EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
