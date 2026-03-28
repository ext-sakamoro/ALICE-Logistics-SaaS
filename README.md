# ALICE-Logistics-SaaS

Supply chain optimization API — part of the ALICE Eco-System.

## Overview

ALICE-Logistics-SaaS provides high-performance supply chain optimization endpoints including route optimization, inventory management, demand forecasting, and vehicle routing problem (VRP) solving.

## Services

- **core-engine** — Domain logic, optimization algorithms, stats (port 8123)
- **api-gateway** — JWT auth, rate limiting, reverse proxy

## Quick Start

```bash
cd services/core-engine
cargo run

# Health check
curl http://localhost:8123/health
```

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | /api/v1/logistics/route | Optimize delivery route |
| POST | /api/v1/logistics/inventory | Inventory reorder analysis |
| POST | /api/v1/logistics/forecast | Demand forecasting |
| POST | /api/v1/logistics/vrp | Vehicle routing problem |
| GET  | /api/v1/logistics/stats | Service statistics |

## License

AGPL-3.0-or-later
