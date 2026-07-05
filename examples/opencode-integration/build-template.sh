#!/usr/bin/env bash
# Copyright (c) 2026 Tencent Inc.
# SPDX-License-Identifier: Apache-2.0

set -euo pipefail

IMAGE="${IMAGE:-opencode-cube:latest}"
WRITABLE_LAYER_SIZE="${WRITABLE_LAYER_SIZE:-4G}"
EXPOSE_PORT="${EXPOSE_PORT:-49983}"
PROBE_PORT="${PROBE_PORT:-49983}"
PROBE_PATH="${PROBE_PATH:-/health}"

cat <<EOF
Build and push the image:

  docker build --platform linux/amd64 -t ${IMAGE} .
  docker push ${IMAGE}

Register the CubeSandbox template:

  cubemastercli tpl create-from-image \\
    --image ${IMAGE} \\
    --writable-layer-size ${WRITABLE_LAYER_SIZE} \\
    --expose-port ${EXPOSE_PORT} \\
    --probe ${PROBE_PORT} \\
    --probe-path ${PROBE_PATH}

After the job reaches READY, copy the returned template_id into CUBE_TEMPLATE_ID.
EOF
