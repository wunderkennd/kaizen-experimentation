FROM golang:1.26-bookworm AS builder
WORKDIR /build
COPY services/ services/
COPY gen/go/ gen/go/
WORKDIR /build/services
RUN CGO_ENABLED=0 go build -o /kaizen-management ./management/cmd/

# Alpine (not distroless): the ECS db-migration task runs this same image
# with an entrypoint override (infra/pkg/aws/compute/migration.go) and needs
# sh + psql + the SQL tree.
FROM alpine:3.21
RUN apk add --no-cache ca-certificates postgresql16-client
COPY --from=builder /kaizen-management /usr/local/bin/kaizen-management
COPY sql/migrations /app/sql/migrations
COPY scripts/run-migrations.sh /app/run-migrations.sh
EXPOSE 50055
CMD ["/usr/local/bin/kaizen-management"]
