# docker/

Dockerfiles for Title Protocol components.

## Images

| Dockerfile | Component | Description |
|-----------|-----------|-------------|
| `gateway.Dockerfile` | Gateway | HTTP API server (axum) |
| `tee-mock.Dockerfile` | TEE (mock) | TEE server with MockRuntime for local development |
| `proxy.Dockerfile` | Proxy | TEE HTTP proxy for external communication |
| `indexer.Dockerfile` | Indexer | cNFT indexer (TypeScript) |

## Usage

These Dockerfiles are used by `docker-compose` configurations:

- **Local development**: `docker compose up` (project root)
- **Production (AWS)**: See `deploy/aws/docker-compose.production.yml` and `deploy/aws/README.md`
