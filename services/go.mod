module github.com/org/experimentation-platform/services

go 1.23.0

require (
	connectrpc.com/connect v1.17.0
	github.com/google/uuid v1.6.0
	github.com/jackc/pgx/v5 v5.7.0
	github.com/org/experimentation/gen/go v0.0.0
	github.com/segmentio/kafka-go v0.4.50
	github.com/stretchr/testify v1.9.0
	golang.org/x/net v0.38.0
	google.golang.org/protobuf v1.35.0
)

require (
	github.com/davecgh/go-spew v1.1.1 // indirect
	github.com/jackc/pgpassfile v1.0.0 // indirect
	github.com/jackc/pgservicefile v0.0.0-20240606120523-5a60cdf6a761 // indirect
	github.com/jackc/puddle/v2 v2.2.1 // indirect
	github.com/klauspost/compress v1.15.9 // indirect
	github.com/kr/text v0.2.0 // indirect
	github.com/pierrec/lz4/v4 v4.1.15 // indirect
	github.com/pmezard/go-difflib v1.0.0 // indirect
	github.com/rogpeppe/go-internal v1.14.1 // indirect
	golang.org/x/crypto v0.36.0 // indirect
	golang.org/x/sync v0.12.0 // indirect
	golang.org/x/text v0.23.0 // indirect
	gopkg.in/yaml.v3 v3.0.1 // indirect
)

replace github.com/org/experimentation/gen/go => ../gen/go
