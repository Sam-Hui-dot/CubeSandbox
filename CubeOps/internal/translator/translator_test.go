// Copyright (c) 2026 Tencent Inc.
// SPDX-License-Identifier: Apache-2.0

package translator

import (
	"encoding/json"
	"testing"
)

func TestTransformSandboxDetailIncludesContainerMetadata(t *testing.T) {
	raw := json.RawMessage(`{
		"ret":{"ret_code":0},
		"data":[{
			"sandbox_id":"sandbox-a",
			"status":1,
			"containers":[
				{"container_id":"main","status":1,"type":"sandbox","image":"main:latest"},
				{"container_id":"stopped","status":2,"type":"sidecar","image":"sidecar:latest"}
			]
		}]
	}`)

	result, ok := TransformSandboxDetail(raw).(map[string]interface{})
	if !ok {
		t.Fatalf("unexpected result type %T", TransformSandboxDetail(raw))
	}
	containers, ok := result["containers"].([]map[string]interface{})
	if !ok || len(containers) != 2 {
		t.Fatalf("unexpected containers: %#v", result["containers"])
	}
	if containers[0]["state"] != "running" {
		t.Fatalf("running container state = %#v", containers[0]["state"])
	}
	if containers[1]["state"] == "running" {
		t.Fatalf("stopped container must not be exposed as running: %#v", containers[1])
	}
}
