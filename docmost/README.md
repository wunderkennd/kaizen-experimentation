# Kaizen Experimentation - DocMost Documentation Site

Self-hosted documentation wiki for the Kaizen Experimentation Platform, powered by [DocMost](https://docmost.com).

## Quick Start

### Prerequisites
- Docker and Docker Compose

### Setup

```bash
cd docmost

# Create environment file from template
cp .env.example .env

# Generate a secret key and strong password, then edit .env:
#   APP_SECRET=$(openssl rand -hex 32)
#   POSTGRES_PASSWORD=$(openssl rand -hex 16)

# Start DocMost
docker compose up -d

# Open http://localhost:3000 in your browser
# Create a workspace named "Kaizen Experimentation"
# Create your admin user account
```

### Populate with Documentation

After creating your workspace and user:

```bash
# Install dependencies
pip install requests

# Log in via API to get a token
curl -X POST http://localhost:3000/api/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"your@email.com","password":"your-password"}'

# Save the token
echo 'YOUR_TOKEN_HERE' > /tmp/docmost_token.txt

# Run the population script
python3 populate_docmost.py
```

## Documentation Structure

The script creates the following spaces in DocMost:

| Space | Content |
|-------|---------|
| **General** | Platform overview, contributing guide, development workflow |
| **Architecture** | System design doc v5.1, Mermaid diagrams, architectural patterns |
| **Modules** | Platform README with module details (M1-M7) |
| **Architecture Decision Records** | All 10 ADRs (language selection, LMAX core, RocksDB, etc.) |
| **Agent Onboarding** | Per-agent quickstart guides (Agent-0 through Agent-7) |
| **Project Coordination** | Status tracker, coordinator playbook, agent prompts |

## Configuration

| Variable | Description | Default |
|----------|-------------|---------|
| `APP_URL` | Public URL for DocMost | `http://localhost:3000` |
| `APP_SECRET` | Secret key for session encryption | (required) |
| `POSTGRES_PASSWORD` | PostgreSQL password | (required) |

## Stopping

```bash
docker compose down          # Stop (preserve data)
docker compose down -v       # Stop and delete all data
```

## Updating

```bash
docker compose pull          # Pull latest DocMost image
docker compose up -d         # Restart with new image
```
