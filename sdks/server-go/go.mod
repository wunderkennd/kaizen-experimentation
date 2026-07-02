module github.com/org/experimentation/sdks/server-go

go 1.25.0

// ADR-031 pilot — initial connectrpc deps for the Go-side round-trip test
// (#641). The full client migration (replacing the hand-rolled JSON shim with
// the generated assignmentv1connect client) lands in #644.
require (
	connectrpc.com/connect v1.17.0
	github.com/org/experimentation/gen/go v0.0.0
)

require (
	golang.org/x/text v0.34.0 // indirect
	google.golang.org/protobuf v1.36.11 // indirect
)

replace github.com/org/experimentation/gen/go => ../../gen/go
