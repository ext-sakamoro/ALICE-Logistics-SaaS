use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Instant,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use uuid::Uuid;

#[derive(Debug, Default)]
struct Stats {
    total_ops: u64,
    route_ops: u64,
    inventory_ops: u64,
    forecast_ops: u64,
    vrp_ops: u64,
}

type AppState = Arc<Mutex<Stats>>;

// --- /api/v1/logistics/route ---
#[derive(Debug, Deserialize)]
struct RouteRequest {
    depot: [f64; 2],
    stops: Vec<[f64; 2]>,
    #[serde(default)]
    vehicle_capacity: f64,
}

#[derive(Debug, Serialize)]
struct RouteResponse {
    request_id: String,
    optimized_order: Vec<usize>,
    total_distance_km: f64,
    estimated_duration_min: f64,
}

async fn route_optimize(
    State(state): State<AppState>,
    Json(req): Json<RouteRequest>,
) -> Result<Json<RouteResponse>, StatusCode> {
    {
        let mut s = state.lock().unwrap();
        s.total_ops += 1;
        s.route_ops += 1;
    }

    let n = req.stops.len();
    let optimized_order: Vec<usize> = (0..n).collect();
    let mut total_distance = 0.0_f64;
    let mut prev = req.depot;
    for &idx in &optimized_order {
        let stop = req.stops[idx];
        let d = ((stop[0] - prev[0]).powi(2) + (stop[1] - prev[1]).powi(2)).sqrt() * 111.0;
        total_distance += d;
        prev = stop;
    }
    // return to depot
    let d = ((req.depot[0] - prev[0]).powi(2) + (req.depot[1] - prev[1]).powi(2)).sqrt() * 111.0;
    total_distance += d;

    Ok(Json(RouteResponse {
        request_id: Uuid::new_v4().to_string(),
        optimized_order,
        total_distance_km: (total_distance * 10.0).round() / 10.0,
        estimated_duration_min: (total_distance / 60.0 * 60.0 * 10.0).round() / 10.0,
    }))
}

// --- /api/v1/logistics/inventory ---
#[derive(Debug, Deserialize)]
struct InventoryRequest {
    sku: String,
    current_stock: u64,
    daily_demand: f64,
    lead_time_days: u32,
    safety_stock_days: u32,
}

#[derive(Debug, Serialize)]
struct InventoryResponse {
    request_id: String,
    sku: String,
    reorder_point: u64,
    reorder_quantity: u64,
    days_of_stock: f64,
    action: String,
}

async fn inventory_analysis(
    State(state): State<AppState>,
    Json(req): Json<InventoryRequest>,
) -> Result<Json<InventoryResponse>, StatusCode> {
    {
        let mut s = state.lock().unwrap();
        s.total_ops += 1;
        s.inventory_ops += 1;
    }

    let reorder_point =
        ((req.lead_time_days + req.safety_stock_days) as f64 * req.daily_demand).ceil() as u64;
    let eoq = (2.0 * req.daily_demand * 365.0 * 50.0 / (req.daily_demand * 0.2)).sqrt();
    let reorder_quantity = eoq.ceil() as u64;
    let days_of_stock = if req.daily_demand > 0.0 {
        req.current_stock as f64 / req.daily_demand
    } else {
        f64::INFINITY
    };
    let action = if req.current_stock <= reorder_point {
        "REORDER_NOW"
    } else {
        "SUFFICIENT"
    }
    .to_string();

    Ok(Json(InventoryResponse {
        request_id: Uuid::new_v4().to_string(),
        sku: req.sku,
        reorder_point,
        reorder_quantity,
        days_of_stock: (days_of_stock * 10.0).round() / 10.0,
        action,
    }))
}

// --- /api/v1/logistics/forecast ---
#[derive(Debug, Deserialize)]
struct ForecastRequest {
    historical_demand: Vec<f64>,
    horizon_days: u32,
    #[serde(default = "default_alpha")]
    alpha: f64,
}

fn default_alpha() -> f64 {
    0.3
}

#[derive(Debug, Serialize)]
struct ForecastResponse {
    request_id: String,
    forecast: Vec<f64>,
    mape: f64,
    method: String,
}

async fn demand_forecast(
    State(state): State<AppState>,
    Json(req): Json<ForecastRequest>,
) -> Result<Json<ForecastResponse>, StatusCode> {
    {
        let mut s = state.lock().unwrap();
        s.total_ops += 1;
        s.forecast_ops += 1;
    }

    if req.historical_demand.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let alpha = req.alpha.clamp(0.01, 0.99);
    let mut smoothed = req.historical_demand[0];
    let mut errors = Vec::new();
    for &v in &req.historical_demand[1..] {
        let pred = smoothed;
        smoothed = alpha * v + (1.0 - alpha) * smoothed;
        if v.abs() > 1e-9 {
            errors.push(((v - pred) / v).abs());
        }
    }
    let mape = if errors.is_empty() {
        0.0
    } else {
        errors.iter().sum::<f64>() / errors.len() as f64 * 100.0
    };

    let forecast: Vec<f64> = (0..req.horizon_days)
        .map(|_| (smoothed * 10.0).round() / 10.0)
        .collect();

    Ok(Json(ForecastResponse {
        request_id: Uuid::new_v4().to_string(),
        forecast,
        mape: (mape * 10.0).round() / 10.0,
        method: "exponential_smoothing".to_string(),
    }))
}

// --- /api/v1/logistics/vrp ---
#[derive(Debug, Deserialize)]
struct VrpRequest {
    vehicles: u32,
    capacity: f64,
    depot: [f64; 2],
    customers: Vec<VrpCustomer>,
}

#[derive(Debug, Deserialize)]
struct VrpCustomer {
    id: String,
    location: [f64; 2],
    demand: f64,
}

#[derive(Debug, Serialize)]
struct VrpResponse {
    request_id: String,
    routes: Vec<VrpRoute>,
    total_distance_km: f64,
    vehicles_used: u32,
}

#[derive(Debug, Serialize)]
struct VrpRoute {
    vehicle_id: u32,
    stops: Vec<String>,
    load: f64,
    distance_km: f64,
}

async fn vrp_solve(
    State(state): State<AppState>,
    Json(req): Json<VrpRequest>,
) -> Result<Json<VrpResponse>, StatusCode> {
    {
        let mut s = state.lock().unwrap();
        s.total_ops += 1;
        s.vrp_ops += 1;
    }

    // greedy bin-packing heuristic
    let mut routes: Vec<VrpRoute> = Vec::new();
    let mut vehicle_id = 0u32;
    let mut current_stops: Vec<String> = Vec::new();
    let mut current_load = 0.0_f64;
    let mut current_dist = 0.0_f64;
    let mut prev = req.depot;

    for customer in &req.customers {
        if current_load + customer.demand > req.capacity && !current_stops.is_empty() {
            let ret = ((req.depot[0] - prev[0]).powi(2) + (req.depot[1] - prev[1]).powi(2))
                .sqrt()
                * 111.0;
            routes.push(VrpRoute {
                vehicle_id,
                stops: std::mem::take(&mut current_stops),
                load: current_load,
                distance_km: (current_dist + ret) * 10.0 / 10.0,
            });
            vehicle_id += 1;
            current_load = 0.0;
            current_dist = 0.0;
            prev = req.depot;
        }
        let d = ((customer.location[0] - prev[0]).powi(2)
            + (customer.location[1] - prev[1]).powi(2))
        .sqrt()
            * 111.0;
        current_dist += d;
        current_load += customer.demand;
        current_stops.push(customer.id.clone());
        prev = customer.location;
    }
    if !current_stops.is_empty() {
        let ret =
            ((req.depot[0] - prev[0]).powi(2) + (req.depot[1] - prev[1]).powi(2)).sqrt() * 111.0;
        routes.push(VrpRoute {
            vehicle_id,
            stops: current_stops,
            load: current_load,
            distance_km: ((current_dist + ret) * 10.0).round() / 10.0,
        });
    }

    let total_distance_km = routes.iter().map(|r| r.distance_km).sum::<f64>();
    let vehicles_used = routes.len() as u32;

    Ok(Json(VrpResponse {
        request_id: Uuid::new_v4().to_string(),
        routes,
        total_distance_km: (total_distance_km * 10.0).round() / 10.0,
        vehicles_used,
    }))
}

// --- /api/v1/logistics/stats ---
#[derive(Debug, Serialize)]
struct StatsResponse {
    service: &'static str,
    version: &'static str,
    total_ops: u64,
    route_ops: u64,
    inventory_ops: u64,
    forecast_ops: u64,
    vrp_ops: u64,
}

async fn get_stats(State(state): State<AppState>) -> Json<StatsResponse> {
    let s = state.lock().unwrap();
    Json(StatsResponse {
        service: "alice-logistics-core",
        version: env!("CARGO_PKG_VERSION"),
        total_ops: s.total_ops,
        route_ops: s.route_ops,
        inventory_ops: s.inventory_ops,
        forecast_ops: s.forecast_ops,
        vrp_ops: s.vrp_ops,
    })
}

// --- /health ---
#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    uptime_secs: u64,
    total_ops: u64,
}

async fn health(
    State(state): State<AppState>,
    axum::extract::Extension(start): axum::extract::Extension<Arc<Instant>>,
) -> Json<HealthResponse> {
    let s = state.lock().unwrap();
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        uptime_secs: start.elapsed().as_secs(),
        total_ops: s.total_ops,
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state: AppState = Arc::new(Mutex::new(Stats::default()));
    let start = Arc::new(Instant::now());

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/logistics/route", post(route_optimize))
        .route("/api/v1/logistics/inventory", post(inventory_analysis))
        .route("/api/v1/logistics/forecast", post(demand_forecast))
        .route("/api/v1/logistics/vrp", post(vrp_solve))
        .route("/api/v1/logistics/stats", get(get_stats))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .layer(axum::extract::Extension(start))
        .with_state(state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8123);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!("alice-logistics-core listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
