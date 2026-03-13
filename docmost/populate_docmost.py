#!/usr/bin/env python3
"""
Populate DocMost with Kaizen Experimentation documentation from the repository.

Usage:
    1. Start DocMost: docker compose up -d
    2. Create a workspace and user via the DocMost UI at http://localhost:3000
    3. Log in via API to get a token:
         curl -X POST http://localhost:3000/api/auth/login \
           -H 'Content-Type: application/json' \
           -d '{"email":"your@email.com","password":"your-password"}'
    4. Save the token: echo 'YOUR_TOKEN' > /tmp/docmost_token.txt
    5. Run this script: python3 populate_docmost.py

Requirements:
    pip install requests
"""

import json
import os
import sys
import time
import requests

BASE_URL = os.environ.get("DOCMOST_URL", "http://localhost:3000")
TOKEN_FILE = os.environ.get("DOCMOST_TOKEN_FILE", "/tmp/docmost_token.txt")
REPO_ROOT = os.environ.get("REPO_ROOT", os.path.join(os.path.dirname(__file__), ".."))

def get_token():
    """Read auth token from file."""
    if os.path.exists(TOKEN_FILE):
        return open(TOKEN_FILE).read().strip()
    print(f"Error: Token file not found at {TOKEN_FILE}")
    print("Please log in via the DocMost API and save the token.")
    sys.exit(1)

AUTH_TOKEN = get_token()
HEADERS = {
    "Authorization": f"Bearer {AUTH_TOKEN}",
    "Content-Type": "application/json",
}


def api_post(endpoint, data):
    """Make a POST request to the DocMost API."""
    resp = requests.post(f"{BASE_URL}/api/{endpoint}", json=data, headers=HEADERS)
    if resp.status_code not in (200, 201):
        print(f"  ERROR {resp.status_code}: {resp.text[:200]}")
        return None
    result = resp.json()
    if result.get("success"):
        return result.get("data")
    print(f"  API error: {result}")
    return None


def create_space(name, slug, description=""):
    """Create a DocMost space."""
    print(f"Creating space: {name}")
    data = api_post("spaces/create", {
        "name": name,
        "slug": slug,
        "description": description,
    })
    if data:
        print(f"  Created space: {data['id']}")
        return data["id"]
    return None


def create_page(space_id, title, content_md, parent_page_id=None):
    """Create a page with markdown content."""
    print(f"  Creating page: {title}")
    payload = {
        "title": title,
        "spaceId": space_id,
        "format": "markdown",
        "content": content_md,
    }
    if parent_page_id:
        payload["parentPageId"] = parent_page_id
    data = api_post("pages/create", payload)
    if data:
        return data["id"]
    return None


def read_file(path):
    """Read a file from the repo."""
    full_path = os.path.join(REPO_ROOT, path)
    if os.path.exists(full_path):
        with open(full_path, "r") as f:
            return f.read()
    return f"*File not found: {path}*"


def populate_general_space(space_id):
    """Populate the General space with overview documentation."""
    welcome_md = """# Kaizen Experimentation Platform

Welcome to the **Kaizen Experimentation Platform** documentation. This is a full-stack experimentation platform for SVOD content optimization, supporting A/B testing, interleaving experiments, multi-armed bandits, and feature flags.

## Platform Overview

| Module | Language | Description |
|--------|----------|-------------|
| M1 Assignment | Rust | Deterministic user bucketing, interleaving, bandit arm delegation |
| M2 Pipeline | Rust + Go | Event ingestion, validation, Kafka publishing |
| M3 Metrics | Go + Spark SQL | Metric computation orchestration |
| M4a Analysis | Rust | Statistical tests (t-test, mSPRT, GST, CUPED, bootstrap) |
| M4b Bandit | Rust | Thompson Sampling, LinUCB, LMAX single-threaded policy core |
| M5 Management | Go | Experiment CRUD, lifecycle state machine, guardrail auto-pause |
| M6 UI | TypeScript | Next.js dashboards (UI only) |
| M7 Flags | Go | Feature flags with experiment promotion via CGo hash bridge |

## Design Principles

- **SVOD-Native**: Streaming-specific experiment types built in, not bolted on
- **Adaptive**: Contextual bandits and content cold-start bandits enable real-time optimization
- **Crash-Only**: Stateless services share startup and recovery code paths
- **Fail-Fast**: Invalid data triggers immediate failure
- **Schema-First**: All interfaces defined in Protobuf with buf toolchain enforcement
- **Guardrails-Default-Safe**: Guardrail breaches auto-pause experiments by default
"""
    create_page(space_id, "Welcome to Kaizen Experimentation", welcome_md)

    # Contributing guide from repo
    contributing_md = read_file("CONTRIBUTING.md")
    create_page(space_id, "Contributing Guide", contributing_md)

    # Development workflow (synthesized from CLAUDE.md)
    dev_md = read_file("CLAUDE.md")
    create_page(space_id, "Development Workflow (CLAUDE.md)", dev_md)


def populate_architecture_space(space_id):
    """Populate Architecture space with design documentation."""
    design_doc = read_file("docs/design/design_doc_v5.1.md")
    create_page(space_id, "System Design Document v5.1", design_doc)

    # Mermaid diagrams as separate pages
    mermaid_files = [
        ("docs/design/system_architecture.mermaid", "System Architecture Diagram"),
        ("docs/design/crate_graph.mermaid", "Cargo Crate Dependency Graph"),
        ("docs/design/data_flow.mermaid", "Data Flow Diagram"),
        ("docs/design/lmax_threading.mermaid", "LMAX Threading Model"),
        ("docs/design/state_machine.mermaid", "Experiment State Machine"),
        ("docs/design/sdk_provider.mermaid", "SDK Provider Architecture"),
    ]
    for filepath, title in mermaid_files:
        content = read_file(filepath)
        if not content.startswith("*File not found"):
            md = f"# {title}\n\n```mermaid\n{content}\n```"
            create_page(space_id, title, md)


def populate_modules_space(space_id):
    """Populate Modules space with per-module documentation."""
    readme_md = read_file("README.md")
    create_page(space_id, "Platform Overview (README)", readme_md)


def populate_adrs_space(space_id):
    """Populate ADR space with architecture decision records."""
    adr_readme = read_file("adrs/README.md")
    create_page(space_id, "ADR Overview", adr_readme)

    adr_files = sorted([
        f for f in os.listdir(os.path.join(REPO_ROOT, "adrs"))
        if f.endswith(".md") and f != "README.md"
    ])
    for adr_file in adr_files:
        content = read_file(f"adrs/{adr_file}")
        title = adr_file.replace(".md", "").replace("-", " ").title()
        for line in content.split("\n"):
            if line.startswith("# "):
                title = line[2:].strip()
                break
        create_page(space_id, title, content)


def populate_onboarding_space(space_id):
    """Populate Onboarding space with agent guides."""
    onboarding_readme = read_file("docs/onboarding/README.md")
    create_page(space_id, "Agent Onboarding Overview", onboarding_readme)

    onboarding_dir = os.path.join(REPO_ROOT, "docs", "onboarding")
    if os.path.isdir(onboarding_dir):
        for f in sorted(os.listdir(onboarding_dir)):
            if f.endswith(".md") and f != "README.md":
                content = read_file(f"docs/onboarding/{f}")
                title = f.replace(".md", "").replace("-", " ").title()
                for line in content.split("\n"):
                    if line.startswith("# "):
                        title = line[2:].strip()
                        break
                create_page(space_id, title, content)


def populate_coordination_space(space_id):
    """Populate Coordination space with status and playbook."""
    status_md = read_file("docs/coordination/status.md")
    create_page(space_id, "Coordination Status", status_md)

    playbook_md = read_file("docs/coordination/playbook.md")
    create_page(space_id, "Coordinator Playbook", playbook_md)

    continuation_md = read_file("docs/coordination/continuation-prompts.md")
    if not continuation_md.startswith("*File not found"):
        create_page(space_id, "Continuation Prompts", continuation_md)

    # Agent prompts
    prompts_dir = os.path.join(REPO_ROOT, "docs", "coordination", "prompts")
    if os.path.isdir(prompts_dir):
        parent = create_page(space_id, "Agent Prompts",
                             "# Agent System Prompts\n\nSystem prompts for each specialized agent.")
        for f in sorted(os.listdir(prompts_dir)):
            if f.endswith(".md"):
                content = read_file(f"docs/coordination/prompts/{f}")
                title = f.replace(".md", "").replace("-", " ").title()
                for line in content.split("\n"):
                    if line.startswith("# "):
                        title = line[2:].strip()
                        break
                create_page(space_id, title, content, parent_page_id=parent)


def main():
    print("=" * 60)
    print("Populating DocMost with Kaizen Experimentation Documentation")
    print("=" * 60)
    print(f"DocMost URL: {BASE_URL}")
    print(f"Repo root:   {REPO_ROOT}")
    print()

    # Create spaces
    general_id = create_space("General", "general", "Overview and getting started")
    arch_id = create_space("Architecture", "architecture", "System design and architectural patterns")
    modules_id = create_space("Modules", "modules", "Module documentation (M1-M7)")
    adr_id = create_space("Architecture Decision Records", "adrs", "ADRs documenting settled decisions")
    onboarding_id = create_space("Agent Onboarding", "onboarding", "Per-agent quickstart guides")
    coord_id = create_space("Project Coordination", "coordination", "Multi-agent coordination")

    # Populate each space
    if general_id:
        populate_general_space(general_id)
    if arch_id:
        populate_architecture_space(arch_id)
    if modules_id:
        populate_modules_space(modules_id)
    if adr_id:
        populate_adrs_space(adr_id)
    if onboarding_id:
        populate_onboarding_space(onboarding_id)
    if coord_id:
        populate_coordination_space(coord_id)

    print("\n" + "=" * 60)
    print("Documentation population complete!")
    print("=" * 60)


if __name__ == "__main__":
    main()
