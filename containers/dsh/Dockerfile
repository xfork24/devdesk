# One complete runtime layer per version/platform, shared by all role/session workers.
FROM node:24.13.0-bookworm-slim@sha256:4660b1ca8b28d6d1906fd644abe34b2ed81d15434d26d845ef0aced307cf4b6f
ARG DSH_VERSION=0.2.2-rc.1
RUN npm install --global --omit=dev @deepseek-ai/dsh@${DSH_VERSION} \
    && dsh --version \
    && npm cache clean --force
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install --yes --no-install-recommends ca-certificates bubblewrap \
    && rm -rf /var/lib/apt/lists/*
COPY devdesk-policy /opt/devdesk/dsh-policy
# Load immutable native addons from the image; the generic writable /tmp stays noexec.
ENV DSH_HOME=/state/dsh DSH_MAX_TOKENS_AS_SUCCESS=false NARB_DISABLE_NATIVE_CACHE=1
USER 1000:1000
WORKDIR /work
ENTRYPOINT ["dsh", "--profile", "sdk"]
