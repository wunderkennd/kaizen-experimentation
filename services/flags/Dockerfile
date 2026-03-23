FROM golang:1.25-bookworm AS builder
WORKDIR /build
COPY services/ services/
COPY gen/go/ gen/go/
WORKDIR /build/services
RUN CGO_ENABLED=0 go build -o /app ./flags/cmd/

FROM gcr.io/distroless/static-debian12
COPY --from=builder /app /app
EXPOSE 50057
CMD ["/app"]
