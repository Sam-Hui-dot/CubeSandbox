// Copyright (c) 2024 Tencent Inc.
// SPDX-License-Identifier: Apache-2.0
//

package cube

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

func TestTerminalWebSocketRequiresInternalRelayHeader(t *testing.T) {
	request := httptest.NewRequest(http.MethodGet, "http://master/cube/sandbox/terminal", nil)
	if terminalUpgrader.CheckOrigin(request) {
		t.Fatal("connection without internal relay header should be rejected")
	}

	request.Header.Set(terminalRelayHeader, terminalRelayHeaderValue)
	if !terminalUpgrader.CheckOrigin(request) {
		t.Fatal("internal CubeAPI relay should be accepted")
	}

	request.Header.Set("Origin", "https://dashboard.example.com")
	if terminalUpgrader.CheckOrigin(request) {
		t.Fatal("browser-originated connection should be rejected even with relay header")
	}
}

func TestValidateTerminalOpenControl(t *testing.T) {
	valid := &terminalOpenControl{
		Type:        "open",
		SandboxID:   " sandbox ",
		ContainerID: " container ",
		Cols:        80,
		Rows:        24,
	}
	if err := validateTerminalOpenControl(valid); err != nil {
		t.Fatalf("valid terminal open rejected: %v", err)
	}
	if valid.SandboxID != "sandbox" || valid.ContainerID != "container" {
		t.Fatalf("identifiers were not normalized: %+v", valid)
	}

	for name, open := range map[string]*terminalOpenControl{
		"missing":           nil,
		"wrong type":        {Type: "resize", SandboxID: "sandbox", ContainerID: "container", Cols: 80, Rows: 24},
		"missing sandbox":   {Type: "open", ContainerID: "container", Cols: 80, Rows: 24},
		"missing container": {Type: "open", SandboxID: "sandbox", Cols: 80, Rows: 24},
		"zero dimension":    {Type: "open", SandboxID: "sandbox", ContainerID: "container", Rows: 24},
		"large dimension":   {Type: "open", SandboxID: "sandbox", ContainerID: "container", Cols: 1001, Rows: 24},
		"relative cwd":      {Type: "open", SandboxID: "sandbox", ContainerID: "container", Cols: 80, Rows: 24, Cwd: "tmp"},
		"invalid env":       {Type: "open", SandboxID: "sandbox", ContainerID: "container", Cols: 80, Rows: 24, Env: []string{"INVALID"}},
	} {
		t.Run(name, func(t *testing.T) {
			if err := validateTerminalOpenControl(open); err == nil {
				t.Fatal("expected validation error")
			}
		})
	}
}
