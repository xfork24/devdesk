# Shared provider/version layer; role instructions and session state are mounted at runtime.
FROM node:24.13.0-bookworm-slim@sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f
ARG CODEX_VERSION=0.167.0
RUN apt-get update && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN npm install --global --omit=dev --ignore-scripts @openai/codex@${CODEX_VERSION} \
    && codex --version \
    && npm cache clean --force
ENV CODEX_HOME=/state/codex
USER 1000:1000
WORKDIR /work
ENTRYPOINT ["codex", "app-server"]
