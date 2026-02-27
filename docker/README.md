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

These Dockerfiles are used by the production Docker Compose configuration:

```bash
docker compose -f deploy/aws/docker-compose.production.yml up -d
```

See `deploy/aws/README.md` for details.
