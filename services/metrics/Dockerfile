FROM golang:1.26-bookworm AS builder
WORKDIR /build
COPY services/ services/
COPY gen/go/ gen/go/
WORKDIR /build/services
RUN CGO_ENABLED=0 go build -o /app ./metrics/cmd/

FROM gcr.io/distroless/static-debian12
COPY --from=builder /app /app
# cmd/main.go falls back to a source-tree relative path when CONFIG_PATH is
# unset — that path doesn't exist in the image, so bake the dev seed in.
COPY services/metrics/internal/config/testdata/seed_config.json /seed_config.json
ENV CONFIG_PATH=/seed_config.json
EXPOSE 50056
CMD ["/app"]
