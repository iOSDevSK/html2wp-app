FROM node:22-bookworm-slim@sha256:d649c27dae7ba0137b3cef5dd75baa422c08dc3d9e3fc0c23dfb172dc3cc6436
ARG CODEX_VERSION=0.157.0
ARG WRANGLER_VERSION=4.136.1
ENV DEBIAN_FRONTEND=noninteractive PLAYWRIGHT_BROWSERS_PATH=/opt/browsers H2WP_HOST=codex PYTHONDONTWRITEBYTECODE=1
RUN apt-get update && apt-get install -y --no-install-recommends python3 python3-pip php-cli php-mysql php-sqlite3 php-gd mariadb-client jq curl bash tar zip unzip rsync ca-certificates git fonts-liberation && rm -rf /var/lib/apt/lists/*
RUN pip3 install --break-system-packages --no-cache-dir playwright==1.55.0 Pillow==11.3.0 numpy==2.2.6 && python3 -m playwright install --with-deps chromium
RUN npm install -g @openai/codex@${CODEX_VERSION}
# The plugin's preview WordPress (test-env.sh: docker compose) and its build
# sandbox drive the host's Docker from the project container.
RUN install -m 0755 -d /etc/apt/keyrings && curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc && chmod a+r /etc/apt/keyrings/docker.asc \
 && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/debian bookworm stable" > /etc/apt/sources.list.d/docker.list \
 && apt-get update && apt-get install -y --no-install-recommends docker-ce-cli docker-compose-plugin && rm -rf /var/lib/apt/lists/*
# Cloudflare Pages deploys only; runs in its own container with the owner's Cloudflare sign-in.
RUN npm install -g wrangler@${WRANGLER_VERSION} && npm cache clean --force
RUN curl -fsSL https://github.com/wp-cli/wp-cli/releases/download/v2.12.0/wp-cli-2.12.0.phar -o /opt/wp-cli.phar && echo 'be928f6b8ca1e8dfb9d2f4b75a13aa4aee0896f8a9a0a1c45cd5d2c98605e6172e6d014dda2e27f88c98befc16c040cbb2bd1bfa121510ea5cdf5f6a30fe8832  /opt/wp-cli.phar' | sha512sum -c - && chmod 755 /opt/wp-cli.phar
# Gutenberg from an HTML theme (the html2wp-to-gutenberg skill): wp-cli on PATH,
# and a pinned WordPress with the SQLite drop-in, so its sandbox needs no network.
RUN printf '#!/bin/sh\nexec php /opt/wp-cli.phar "$@"\n' > /usr/local/bin/wp && chmod 755 /usr/local/bin/wp
RUN mkdir -p /opt/wp-offline && cd /opt/wp-offline \
 && curl -fsSL https://wordpress.org/wordpress-7.0.2.tar.gz -o wp.tar.gz && echo 'd4a4d219dea64c6c68e62f2ffc3f331c5475510246d6c44f61fb52f377d295b9  wp.tar.gz' | sha256sum -c - && tar xzf wp.tar.gz && rm wp.tar.gz \
 && curl -fsSL https://downloads.wordpress.org/plugin/sqlite-database-integration.3.0.2.zip -o sqlite.zip && echo '1602e75577ad9b3a7e3e4a6a44a81b9541cdee2124d48928faf61c6fd3cd4f74  sqlite.zip' | sha256sum -c - \
 && unzip -q sqlite.zip -d wordpress/wp-content/plugins/ && rm sqlite.zip && chmod -R a+rX /opt/wp-offline
RUN usermod -l agent node && mkdir -p /home/agent/.codex /home/agent/empty && chown -R 1000:1000 /home/agent && chmod 700 /home/agent/.codex
COPY . /opt/desktop
ENV HOME=/home/agent CODEX_HOME=/home/agent/.codex
WORKDIR /home/agent/empty
CMD ["sleep","infinity"]
