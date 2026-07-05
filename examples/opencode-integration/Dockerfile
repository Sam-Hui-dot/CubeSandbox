# syntax=docker/dockerfile:1.7
#
# OpenCode inside a CubeSandbox template.
#
# The inherited cube-entrypoint keeps envd running on :49983 and serves
# /health, so Cube can use the standard readiness probe.

ARG CUBE_BASE_IMAGE=ghcr.io/tencentcloud/cubesandbox-base:2026.16
FROM ${CUBE_BASE_IMAGE}

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_MAJOR=22
ARG OPENCODE_VERSION=1.17.13

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash \
        ca-certificates \
        curl \
        git \
        gnupg \
        jq \
        less \
        procps \
        python3 \
        python3-pip \
        ripgrep \
    && curl -fsSL "https://deb.nodesource.com/setup_${NODE_MAJOR}.x" | bash - \
    && apt-get install -y --no-install-recommends nodejs \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g "opencode-ai@${OPENCODE_VERSION}" \
    && opencode --version \
    && npm cache clean --force \
    && rm -rf /root/.npm

ENV OPENCODE_DISABLE_AUTO_UPDATE=1 \
    NPM_CONFIG_UPDATE_NOTIFIER=false

RUN mkdir -p /workspace /root/.local/share/opencode /root/.config/opencode

WORKDIR /workspace

EXPOSE 49983
