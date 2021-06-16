//! Ticker implementation for dev environment
//!
//! Implements coinmarketcap API for tokens deployed using `deploy-dev-erc20`
//! Prices are randomly distributed around base values estimated from real world prices.

use actix_cors::Cors;
use actix_web::{middleware, web, App, HttpRequest, HttpResponse, HttpServer, Result};
use bigdecimal::BigDecimal;
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::HashMap, fs::File, io::BufReader, path::Path};
use std::{convert::TryFrom, time::Duration};
use structopt::StructOpt;
use zksync_crypto::rand::{thread_rng, Rng};

#[derive(Debug, Serialize, Deserialize)]
struct CoinMarketCapTokenQuery {
    symbol: String,
}

macro_rules! make_sloppy {
    ($f: ident) => {{
        |query| async {
            if thread_rng().gen_range(0, 100) < 5 {
                vlog::debug!("`{}` has been errored", stringify!($f));
                return Ok(HttpResponse::InternalServerError().finish());
            }

            let duration = match thread_rng().gen_range(0, 100) {
                0..=59 => Duration::from_millis(100),
                60..=69 => Duration::from_secs(5),
                _ => {
                    let ms = thread_rng().gen_range(100, 1000);
                    Duration::from_millis(ms)
                }
            };

            vlog::debug!(
                "`{}` has been delayed for {}ms",
                stringify!($f),
                duration.as_millis()
            );
            tokio::time::delay_for(duration).await;

            let resp = $f(query).await;
            resp
        }
    }};
}

async fn handle_coinmarketcap_token_price_query(
    query: web::Query<CoinMarketCapTokenQuery>,
) -> Result<HttpResponse> {
    let symbol = query.symbol.clone();
    let base_price = match symbol.as_str() {
        "ETH" => BigDecimal::from(200),
        "wBTC" => BigDecimal::from(9000),
        "BAT" => BigDecimal::try_from(0.2).unwrap(),
        "DAI" => BigDecimal::from(1),
        "tGLM" => BigDecimal::from(1),
        "GLM" => BigDecimal::from(1),
        _ => BigDecimal::from(0),
    };
    let random_multiplier = thread_rng().gen_range(0.9, 1.1);

    let price = base_price * BigDecimal::try_from(random_multiplier).unwrap();

    let last_updated = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    let resp = json!({
        "data": {
            symbol: {
                "quote": {
                    "USD": {
                        "price": price.to_string(),
                        "last_updated": last_updated
                    }
                }
            }
        }
    });
    vlog::info!("1.0 {} = {} USD", query.symbol, price);
    Ok(HttpResponse::Ok().json(resp))
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TokenData {
    id: String,
    symbol: String,
    name: String,
    platforms: HashMap<String, String>,
}

fn load_tokens(path: impl AsRef<Path>) -> Vec<TokenData> {
    let file = File::open(path).unwrap();
    let reader = BufReader::new(file);

    let values: Vec<HashMap<String, serde_json::Value>> = serde_json::from_reader(reader).unwrap();
    let tokens: Vec<TokenData> = values
        .into_iter()
        .map(|value| {
            let symbol = value["symbol"].as_str().unwrap().to_ascii_lowercase();
            let address = value["address"].as_str().unwrap().to_ascii_lowercase();
            let mut platforms = HashMap::new();
            platforms.insert(String::from("ethereum"), address);
            let id = match symbol.as_str() {
                "eth" => String::from("ethereum"),
                "wbtc" => String::from("wrapped-bitcoin"),
                "bat" => String::from("basic-attention-token"),
                _ => symbol.clone(),
            };

            TokenData {
                id,
                symbol: symbol.clone(),
                name: symbol,
                platforms,
            }
        })
        .collect();
    tokens
}

async fn handle_coingecko_token_list(_req: HttpRequest) -> Result<HttpResponse> {
    let data = load_tokens(&"etc/tokens/localhost.json");
    Ok(HttpResponse::Ok().json(data))
}

async fn handle_coingecko_token_price_query(req: HttpRequest) -> Result<HttpResponse> {
    let coin_id = req.match_info().get("coin_id");
    let base_price = match coin_id {
        Some("ethereum") => BigDecimal::from(200),
        Some("wrapped-bitcoin") => BigDecimal::from(9000),
        Some("basic-attention-token") => BigDecimal::try_from(0.2).unwrap(),
        _ => BigDecimal::from(1),
    };
    let random_multiplier = thread_rng().gen_range(0.9, 1.1);
    let price = base_price * BigDecimal::try_from(random_multiplier).unwrap();

    let last_updated = Utc::now().timestamp_millis();
    let resp = json!({
        "prices": [
            [last_updated, price],
        ]
    });
    vlog::info!("1.0 {:?} = {} USD", coin_id, price);
    Ok(HttpResponse::Ok().json(resp))
}

fn main_scope(sloppy_mode: bool) -> actix_web::Scope {
    if sloppy_mode {
        web::scope("/")
            .route(
                "/cryptocurrency/quotes/latest",
                web::get().to(make_sloppy!(handle_coinmarketcap_token_price_query)),
            )
            .route(
                "/api/v3/coins/list",
                web::get().to(make_sloppy!(handle_coingecko_token_list)),
            )
            .route(
                "/api/v3/coins/{coin_id}/market_chart",
                web::get().to(make_sloppy!(handle_coingecko_token_price_query)),
            )
    } else {
        web::scope("/")
            .route(
                "/cryptocurrency/quotes/latest",
                web::get().to(handle_coinmarketcap_token_price_query),
            )
            .route(
                "/api/v3/coins/list",
                web::get().to(handle_coingecko_token_list),
            )
            .route(
                "/api/v3/coins/{coin_id}/market_chart",
                web::get().to(handle_coingecko_token_price_query),
            )
    }
}

/// Ticker implementation for dev environment
///
/// Implements coinmarketcap API for tokens deployed using `deploy-dev-erc20`
/// Prices are randomly distributed around base values estimated from real world prices.
#[derive(Debug, StructOpt, Clone, Copy)]
struct FeeTickerOpts {
    /// Activate "sloppy" mode.
    ///
    /// With the option, server will provide a random delay for requests
    /// (60% of 0.1 delay, 30% of 0.1 - 1.0 delay, 10% of 5 seconds delay),
    /// and will randomly return errors for 5% of requests.
    #[structopt(long)]
    sloppy: bool,
}

fn main() {
    vlog::init();

    let opts = FeeTickerOpts::from_args();
    if opts.sloppy {
        vlog::info!("Fee ticker server will run in a sloppy mode.");
    }

    let mut runtime = actix_rt::System::new("dev-ticker");
    runtime.block_on(async move {
        HttpServer::new(move || {
            App::new()
                .wrap(middleware::Logger::default())
                .wrap(Cors::new().send_wildcard().max_age(3600).finish())
                .service(main_scope(opts.sloppy))
        })
        .bind("0.0.0.0:9876")
        .unwrap()
        .shutdown_timeout(1)
        .run()
        .await
        .expect("Server crashed");
    });
}
