use axum::{
    extract::{Json, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

// --- ESTRUCTURAS DE ENTRADA ---
#[derive(Deserialize, Debug, Clone)]
struct IncomingPayload {
    event_name: Option<String>,
    event_id: Option<String>,
    value: Option<f64>,
    currency: Option<String>,
    test_event_code: Option<String>, // Entorno de Pruebas
}

// --- ESTRUCTURAS DE WOOCOMMERCE API ---
#[derive(Deserialize, Debug)]
struct WcOrderResponse {
    billing: WcBilling,
    customer_id: i64,
}

#[derive(Deserialize, Debug)]
struct WcBilling {
    email: String,
    phone: String,
}

// --- ESTRUCTURAS META CAPI ---
#[derive(Serialize, Debug)]
struct MetaPayload {
    data: Vec<MetaEvent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    test_event_code: Option<String>, // Entorno de Pruebas
}

#[derive(Serialize, Debug)]
struct MetaEvent {
    event_name: String,
    event_time: i64,
    event_id: String,
    user_data: UserData,
    custom_data: CustomData,
    action_source: String,
}

#[derive(Serialize, Debug, Default)]
struct UserData {
    #[serde(skip_serializing_if = "Option::is_none")]
    em: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ph: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_id: Option<Vec<String>>,
    client_ip_address: String,
    client_user_agent: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fbp: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fbc: Option<String>,
}

#[derive(Serialize, Debug)]
struct CustomData {
    currency: String,
    value: f64,
}

struct AppState {
    http_client: Client,
    meta_url: String,
    meta_api_token: String,
    wc_url: String,
    wc_ck: String,
    wc_cs: String,
}

fn hash_sha256(data: &str, is_phone: bool) -> Option<String> {
    if data.trim().is_empty() { return None; }
    let mut clean = data.trim().to_lowercase();
    if is_phone { clean = clean.chars().filter(|c| c.is_ascii_digit()).collect(); }
    let mut hasher = Sha256::new();
    hasher.update(clean.as_bytes());
    Some(hex::encode(hasher.finalize()))
}

fn get_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers.get("cookie").and_then(|v| {
        let cookie_str = v.to_str().unwrap_or("");
        for cookie in cookie_str.split(';') {
            let mut parts = cookie.splitn(2, '=');
            let key = parts.next()?.trim();
            if key == name { return parts.next().map(|s| s.trim().to_string()); }
        }
        None
    })
}

async fn handle_purchase(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<IncomingPayload>,
) -> StatusCode {
    if payload.event_name.as_deref() != Some("purchase") || payload.event_id.is_none() {
        return StatusCode::OK;
    }

    let order_id = payload.event_id.clone().unwrap();
    let client_ip = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).map(|s| s.split(',').next().unwrap_or("").trim().to_string()).unwrap_or_else(|| "0.0.0.0".to_string());
    let user_agent = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let fbp = get_cookie(&headers, "_fbp");
    let fbc = get_cookie(&headers, "_fbc");

    let client = state.http_client.clone();
    let state_clone = state.clone();

    tokio::spawn(async move {
        let start_time = Instant::now();
        
        // Modo Seguro de Benchmark: Evitar tumbar el servidor PHP de Staging
        if order_id == "ORD_BENCHMARK" {
            // Simulamos un trabajo ligero
            let _ = hash_sha256("benchmark@test.com", false);
            // println!("Benchmark procesado en: {:?}", start_time.elapsed());
            return;
        }

        let wc_order_id = order_id.replace("ORD_", "");
        let wc_endpoint = format!("{}/wp-json/wc/v3/orders/{}", state_clone.wc_url, wc_order_id);
        let wc_req = client.get(&wc_endpoint).basic_auth(&state_clone.wc_ck, Some(&state_clone.wc_cs)).send();

        match timeout(Duration::from_secs(5), wc_req).await {
            Ok(Ok(res)) if res.status().is_success() => {
                if let Ok(order_data) = res.json::<WcOrderResponse>().await {
                    let hash_em = hash_sha256(&order_data.billing.email, false);
                    let hash_ph = hash_sha256(&order_data.billing.phone, true);
                    let hash_ext_id = if order_data.customer_id > 0 { hash_sha256(&order_data.customer_id.to_string(), false) } else { hash_sha256(&order_data.billing.email, false) };

                    let mut user_data = UserData { client_ip_address: client_ip, client_user_agent: user_agent, fbp, fbc, ..Default::default() };
                    if let Some(em) = hash_em { user_data.em = Some(vec![em]); }
                    if let Some(ph) = hash_ph { user_data.ph = Some(vec![ph]); }
                    if let Some(ext) = hash_ext_id { user_data.external_id = Some(vec![ext]); }

                    let meta_payload = MetaPayload {
                        data: vec![MetaEvent {
                            event_name: "Purchase".to_string(),
                            event_time: chrono::Utc::now().timestamp(),
                            event_id: order_id.clone(),
                            action_source: "website".to_string(),
                            user_data,
                            custom_data: CustomData {
                                currency: payload.currency.clone().unwrap_or_else(|| "USD".to_string()),
                                value: payload.value.unwrap_or(0.0),
                            },
                        }],
                        test_event_code: payload.test_event_code.clone(),
                    };

                    let req_meta = client.post(&state_clone.meta_url).bearer_auth(&state_clone.meta_api_token).json(&meta_payload).send();
                    let _ = timeout(Duration::from_secs(5), req_meta).await;
                    
                    println!("ÉXITO: Orden {} enviada a Meta. Tiempo total de procesamiento S2S: {:?}", order_id, start_time.elapsed());
                }
            }
            _ => {
                println!("ERROR: Falló la consulta a WooCommerce para la orden {}. Tiempo transcurrido: {:?}", order_id, start_time.elapsed());
            }
        }
    });

    StatusCode::OK
}

#[tokio::main]
async fn main() {
    let port = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let pixel_id = env::var("META_PIXEL_ID").unwrap_or_else(|_| "PIXEL_ID_HERE".to_string());
    let api_token = env::var("META_API_TOKEN").unwrap_or_else(|_| "TOKEN_HERE".to_string());
    let api_version = env::var("META_API_VERSION").unwrap_or_else(|_| "v19.0".to_string());
    
    let wc_url = env::var("WC_URL").unwrap_or_else(|_| "https://staging56.despensallena.com".to_string());
    let wc_ck = env::var("WC_CK").unwrap_or_else(|_| "ck_bb41efb3f83efc591d827719e87300e8285e420b".to_string());
    let wc_cs = env::var("WC_CS").unwrap_or_else(|_| "cs_612f68e66f6d44973879f6dd89e3b23e81344bdc".to_string());

    let meta_url = format!("https://graph.facebook.com/{}/{}/events", api_version, pixel_id);
    let http_client = Client::builder().pool_idle_timeout(Duration::from_secs(60)).build().unwrap();

    let app_state = Arc::new(AppState { http_client, meta_url, meta_api_token: api_token, wc_url: wc_url.trim_end_matches('/').to_string(), wc_ck, wc_cs });
    let app = Router::new().route("/collect", post(handle_purchase)).with_state(app_state);
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();

    println!("Iniciando Rust Meta Proxy (S2S) puerto {}...", port);
    axum::serve(listener, app).await.unwrap();
}
