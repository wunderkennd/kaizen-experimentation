FROM node:20-alpine AS builder
WORKDIR /app
COPY ui/package*.json ./
RUN npm ci
COPY ui/ .
COPY gen/ts/ ../gen/ts/
RUN npm run build

FROM node:20-alpine
WORKDIR /app
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
EXPOSE 3000
CMD ["node", "server.js"]
