// Copyright (c) 2024 Tencent Inc.
// SPDX-License-Identifier: Apache-2.0
//

package cubebox

import (
	"bytes"
	"context"
	"errors"
	"io"
	"sync"
	"testing"

	containerd "github.com/containerd/containerd/v2/client"
	"github.com/containerd/containerd/v2/pkg/namespaces"
	api "github.com/tencentcloud/CubeSandbox/Cubelet/api/services/cubebox/v1"
	cubeboxstore "github.com/tencentcloud/CubeSandbox/Cubelet/pkg/store/cubebox"
)

type fakeTerminalStream struct {
	mu       sync.Mutex
	received []*api.TerminalMessage
	sent     []*api.TerminalMessage
}

func (f *fakeTerminalStream) Send(message *api.TerminalMessage) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.sent = append(f.sent, message)
	return nil
}

func (f *fakeTerminalStream) Recv() (*api.TerminalMessage, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if len(f.received) == 0 {
		return nil, io.EOF
	}
	message := f.received[0]
	f.received = f.received[1:]
	return message, nil
}

type fakeTerminalProcess struct {
	cols uint32
	rows uint32
}

type fakeTerminalCleanupProcess struct {
	closeIOCalls    int
	deleteCalls     int
	closeNamespace  string
	deleteNamespace string
}

func (f *fakeTerminalCleanupProcess) CloseIO(ctx context.Context, _ ...containerd.IOCloserOpts) error {
	f.closeIOCalls++
	f.closeNamespace, _ = namespaces.Namespace(ctx)
	return nil
}

func (f *fakeTerminalCleanupProcess) Delete(ctx context.Context, _ ...containerd.ProcessDeleteOpts) (*containerd.ExitStatus, error) {
	f.deleteCalls++
	f.deleteNamespace, _ = namespaces.Namespace(ctx)
	return nil, nil
}

func (f *fakeTerminalProcess) Resize(_ context.Context, cols, rows uint32) error {
	f.cols = cols
	f.rows = rows
	return nil
}

func TestValidateTerminalOpen(t *testing.T) {
	for name, open := range map[string]*api.TerminalOpen{
		"missing open":       nil,
		"missing sandbox":    {ContainerId: "container"},
		"missing container":  {SandboxId: "sandbox"},
		"partial size":       {SandboxId: "sandbox", ContainerId: "container", Cols: 80},
		"oversized terminal": {SandboxId: "sandbox", ContainerId: "container", Cols: 1001, Rows: 24},
		"relative cwd":       {SandboxId: "sandbox", ContainerId: "container", Cols: 80, Rows: 24, Cwd: "tmp"},
		"invalid env":        {SandboxId: "sandbox", ContainerId: "container", Cols: 80, Rows: 24, Env: []string{"INVALID"}},
	} {
		t.Run(name, func(t *testing.T) {
			if err := validateTerminalOpen(open); err == nil {
				t.Fatal("expected validation error")
			}
		})
	}
	if err := validateTerminalOpen(&api.TerminalOpen{SandboxId: "sandbox", ContainerId: "container", Cols: 80, Rows: 24}); err != nil {
		t.Fatalf("valid open rejected: %v", err)
	}
}

func TestTerminalCleanupContextPreservesNamespace(t *testing.T) {
	ctx, cancel := terminalCleanupContext("sandbox-namespace")
	defer cancel()

	if namespace, ok := namespaces.Namespace(ctx); !ok || namespace != "sandbox-namespace" {
		t.Fatalf("cleanup namespace = %q, %v; want sandbox-namespace, true", namespace, ok)
	}
}

func TestTerminalResolveExecContainerSelectsRequestedContainer(t *testing.T) {
	main := &cubeboxstore.Container{Metadata: cubeboxstore.Metadata{ID: "ctr-a"}}
	sidecar := &cubeboxstore.Container{Metadata: cubeboxstore.Metadata{ID: "ctr-b"}}
	sandbox := &cubeboxstore.CubeBox{}
	sandbox.AddContainer(main)
	sandbox.AddContainer(sidecar)

	gotMain, err := resolveExecContainer(sandbox, "ctr-a")
	if err != nil || gotMain != main {
		t.Fatalf("ctr-a resolved to %p, %v; want %p", gotMain, err, main)
	}
	gotSidecar, err := resolveExecContainer(sandbox, "ctr-b")
	if err != nil || gotSidecar != sidecar {
		t.Fatalf("ctr-b resolved to %p, %v; want %p", gotSidecar, err, sidecar)
	}
	if gotMain == gotSidecar {
		t.Fatal("different container IDs resolved to the same container handle")
	}
	if _, err := resolveExecContainer(sandbox, "missing"); err == nil {
		t.Fatal("missing container should be rejected")
	}
}

func TestTerminalCleanupDeletesOnlyItsOwnExec(t *testing.T) {
	first := &fakeTerminalCleanupProcess{}
	second := &fakeTerminalCleanupProcess{}

	cleanupTerminalProcess(context.Background(), "sandbox-ns", "exec-a", first)

	if first.closeIOCalls != 1 || first.deleteCalls != 1 {
		t.Fatalf("first process cleanup calls = CloseIO %d, Delete %d; want 1, 1", first.closeIOCalls, first.deleteCalls)
	}
	if first.closeNamespace != "sandbox-ns" || first.deleteNamespace != "sandbox-ns" {
		t.Fatalf("cleanup namespaces = %q, %q; want sandbox-ns", first.closeNamespace, first.deleteNamespace)
	}
	if second.closeIOCalls != 0 || second.deleteCalls != 0 {
		t.Fatalf("unrelated process was cleaned up: CloseIO %d, Delete %d", second.closeIOCalls, second.deleteCalls)
	}
}

func TestTerminalEnvDefaultsTermAndPreservesExplicitValue(t *testing.T) {
	input := []string{"PATH=/usr/bin"}
	withDefault := terminalEnv(input)
	if !equalStrings(withDefault, []string{"PATH=/usr/bin", "TERM=xterm-256color", "LANG=C.UTF-8", "LC_ALL=C.UTF-8"}) {
		t.Fatalf("unexpected default terminal env: %+v", withDefault)
	}
	if len(input) != 1 {
		t.Fatalf("terminalEnv mutated its input: %+v", input)
	}

	explicit := terminalEnv([]string{"TERM=screen-256color", "LANG=C.UTF-8"})
	if len(explicit) != 3 || explicit[0] != "TERM=screen-256color" || explicit[2] != "LC_ALL=C.UTF-8" {
		t.Fatalf("explicit terminal env was not preserved: %+v", explicit)
	}
}

func TestReceiveTerminalCommandsQueuesInputResizesAndCloses(t *testing.T) {
	stream := &fakeTerminalStream{received: []*api.TerminalMessage{
		{Message: &api.TerminalMessage_Input{Input: []byte("echo ok\n")}},
		{Message: &api.TerminalMessage_Resize{Resize: &api.TerminalResize{Cols: 132, Rows: 43}}},
		{Message: &api.TerminalMessage_Close{Close: &api.TerminalClose{}}},
	}}
	process := &fakeTerminalProcess{}
	input := make(chan []byte, 1)

	err := receiveTerminalCommands(context.Background(), stream, process, input)
	if !errors.Is(err, errTerminalClientClosed) {
		t.Fatalf("expected client close, got %v", err)
	}
	if payload := <-input; !bytes.Equal(payload, []byte("echo ok\n")) {
		t.Fatalf("unexpected stdin payload %q", payload)
	}
	if process.cols != 132 || process.rows != 43 {
		t.Fatalf("resize = %dx%d, want 132x43", process.cols, process.rows)
	}
}

func TestReceiveTerminalCommandsRejectsFullInputQueue(t *testing.T) {
	stream := &fakeTerminalStream{received: []*api.TerminalMessage{
		{Message: &api.TerminalMessage_Input{Input: []byte("second")}},
	}}
	input := make(chan []byte, 1)
	input <- []byte("first")

	err := receiveTerminalCommands(context.Background(), stream, &fakeTerminalProcess{}, input)
	if err == nil || err.Error() != "terminal input backlog exceeded" {
		t.Fatalf("expected bounded queue error, got %v", err)
	}
}

func TestCopyTerminalOutputSendsBinaryPayload(t *testing.T) {
	stream := &fakeTerminalStream{}
	sender := &terminalStreamSender{stream: stream}
	err := copyTerminalOutput(bytes.NewBufferString("\x1b[32mok\x1b[0m"), sender)
	if !errors.Is(err, io.EOF) {
		t.Fatalf("expected EOF, got %v", err)
	}
	if len(stream.sent) != 1 || string(stream.sent[0].GetOutput()) != "\x1b[32mok\x1b[0m" {
		t.Fatalf("unexpected output messages: %+v", stream.sent)
	}
}

func TestTerminalConcurrentOutputsDoNotCrossStreams(t *testing.T) {
	streamA := &fakeTerminalStream{}
	streamB := &fakeTerminalStream{}
	start := make(chan struct{})
	errorCh := make(chan error, 2)

	go func() {
		<-start
		errorCh <- copyTerminalOutput(bytes.NewBufferString("marker-a"), &terminalStreamSender{stream: streamA})
	}()
	go func() {
		<-start
		errorCh <- copyTerminalOutput(bytes.NewBufferString("marker-b"), &terminalStreamSender{stream: streamB})
	}()
	close(start)

	for range 2 {
		if err := <-errorCh; !errors.Is(err, io.EOF) {
			t.Fatalf("copyTerminalOutput returned %v, want EOF", err)
		}
	}
	if len(streamA.sent) != 1 || string(streamA.sent[0].GetOutput()) != "marker-a" {
		t.Fatalf("stream A received crossed output: %+v", streamA.sent)
	}
	if len(streamB.sent) != 1 || string(streamB.sent[0].GetOutput()) != "marker-b" {
		t.Fatalf("stream B received crossed output: %+v", streamB.sent)
	}
}

func TestMergeExecEnvOverridesInPlaceAndKeepsOrder(t *testing.T) {
	base := []string{"PATH=/usr/bin", "TERM=dumb", "LANG=C"}
	got := mergeExecEnv(base, []string{"TERM=xterm-256color", "NEW=value", "invalid"})
	want := []string{"PATH=/usr/bin", "TERM=xterm-256color", "LANG=C", "NEW=value"}
	if !equalStrings(got, want) {
		t.Fatalf("merged env = %+v, want %+v", got, want)
	}
	if base[1] != "TERM=dumb" {
		t.Fatalf("mergeExecEnv mutated base: %+v", base)
	}
}

func equalStrings(left, right []string) bool {
	if len(left) != len(right) {
		return false
	}
	for index := range left {
		if left[index] != right[index] {
			return false
		}
	}
	return true
}
